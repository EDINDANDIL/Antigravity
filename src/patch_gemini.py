"""
Gemini CLI patcher for Antigravity Unlocker.
Patches CodeAssist eligibility checks so OAuth-authenticated users
in sanctioned regions can use the CLI.
AIzaSy-key users (gemini-api-key auth) don't need these patches.
"""
import os
import sys
import subprocess
import json

appdata = os.environ.get("APPDATA", "")
if not appdata:
    print("ERROR: APPDATA not defined", file=sys.stderr)
    sys.exit(1)

cli_dir = os.path.join(appdata, "npm", "node_modules", "@google", "gemini-cli")
bundle_dir = os.path.join(cli_dir, "bundle")

# Check state of the bundle files to decide next step
needs_reinstall = False
is_patched = False
if os.path.exists(bundle_dir):
    for fname in os.listdir(bundle_dir):
        if not fname.endswith(".js"):
            continue
        fpath = os.path.join(bundle_dir, fname)
        try:
            with open(fpath, "r", encoding="utf-8") as f:
                content = f.read()
        except Exception:
            continue
        # Old/broken patches → reinstall clean
        if "antigravity-vertex-api" in content or '\\"\\"' in content:
            needs_reinstall = True
            break
        # Correct replacement already applied → skip entirely
        if 'projectId || ""' in content:
            is_patched = True

if needs_reinstall:
    print("Detected old patches. Reinstalling clean @google/gemini-cli first...")
    try:
        import shutil
        if os.path.exists(cli_dir):
            shutil.rmtree(cli_dir)
    except Exception:
        pass
    subprocess.run(["npm", "install", "-g", "@google/gemini-cli@latest"], shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
elif is_patched:
    # Correct replacement found - only apply if some targets still remain
    pass

# DO NOT force selectedType — let the user keep their existing auth method.
# For AIzaSy keys: "gemini-api-key" works out of the box.
# For AQ. tokens: sign in via Google OAuth, then these patches handle region blocks.

# ── Patch JS bundle files ─────────────────────────────────────────────
# IMPORTANT: inside triple-quoted strings, use plain " for JS quotes.
# \\" would produce \" which does NOT match the original JS code.

# Patch 1a: loadCodeAssist — rewrite try block to catch ineligible tiers
t_loadCodeAssist_try = """      return await this.requestPost(
        "loadCodeAssist",
        req
      );"""

r_loadCodeAssist_try = """      const res = await this.requestPost(
        "loadCodeAssist",
        req
      );
      if (res) {
        if (!res.currentTier && res.ineligibleTiers && res.ineligibleTiers.length > 0) {
          res.currentTier = { id: UserTierId.STANDARD, hasOnboardedPreviously: true };
          delete res.ineligibleTiers;
        }
        if (!res.currentTier) {
          res.currentTier = { id: UserTierId.STANDARD, hasOnboardedPreviously: true };
        }
        if (!res.paidTier) {
          res.paidTier = { id: "pro", name: "Pro", availableCredits: [{ creditType: "G1_CREDIT", creditAmount: "99999" }] };
        }
      }
      return res;"""

# Patch 1b: loadCodeAssist — replace project-specific 403 with a generic handler
t_loadCodeAssist_catch = """      } else if (isPermissionDeniedError(e2) && req.cloudaicompanionProject === "cloudshell-gca") {
        throw new Error("Access to the default Cloud Shell Gemini project was denied.\\nPlease set your own Google Cloud project by running:\\ngcloud config set project [PROJECT_ID]\\nor setting export GOOGLE_CLOUD_PROJECT=...");
      } else {
        throw e2;
      }"""

r_loadCodeAssist_catch = """      } else if (isPermissionDeniedError(e2) || (Array.isArray(e2) && e2.length > 0 && (isPermissionDeniedError(e2[0]) || isPermissionDeniedError(e2[0]?.error)))) {
        return {
          currentTier: { id: UserTierId.STANDARD, hasOnboardedPreviously: true },
          paidTier: { id: "pro", name: "Pro", availableCredits: [{ creditType: "G1_CREDIT", creditAmount: "99999" }] },
          cloudaicompanionProject: req.cloudaicompanionProject || ""
        };
      } else {
        throw e2;
      }"""

# Patch 2: listExperiments — return empty instead of throwing when no projectId
t_listExperiments = """  async listExperiments(metadata2) {
    if (!this.projectId) {
      throw new Error("projectId is not defined for CodeAssistServer.");
    }"""

r_listExperiments = """  async listExperiments(metadata2) {
    if (!this.projectId) {
      return { flags: [], experimentIds: [] };
    }"""

# Patch 3: setupUser first location — remove hardcoded project fallback
t_setupUser1 = """    if (!loadRes.cloudaicompanionProject) {
      if (projectId) {
        return {
          projectId,
          userTier: loadRes.paidTier?.id ?? loadRes.currentTier.id ?? UserTierId.STANDARD,
          userTierName: loadRes.paidTier?.name ?? loadRes.currentTier.name,
          paidTier: loadRes.paidTier ?? void 0,
          hasOnboardedPreviously: loadRes.currentTier.hasOnboardedPreviously ?? true
        };
      }
      throwIneligibleOrProjectIdError(loadRes);
    }"""

r_setupUser1 = """    if (!loadRes.cloudaicompanionProject) {
      return {
        projectId: projectId || "",
        userTier: loadRes.paidTier?.id ?? loadRes.currentTier.id ?? UserTierId.STANDARD,
        userTierName: loadRes.paidTier?.name ?? loadRes.currentTier.name,
        paidTier: loadRes.paidTier ?? void 0,
        hasOnboardedPreviously: loadRes.currentTier.hasOnboardedPreviously ?? true
      };
    }"""

# Patch 4: setupUser second location — same fix
t_setupUser2 = """  if (!lroRes.response?.cloudaicompanionProject?.id) {
    if (projectId) {
      return {
        projectId,
        userTier: tier.id ?? UserTierId.STANDARD,
        userTierName: tier.name,
        hasOnboardedPreviously: tier.hasOnboardedPreviously ?? false
      };
    }
    throwIneligibleOrProjectIdError(loadRes);
  }"""

r_setupUser2 = """  if (!lroRes.response?.cloudaicompanionProject?.id) {
    return {
      projectId: projectId || "",
      userTier: tier.id ?? UserTierId.STANDARD,
      userTierName: tier.name,
      hasOnboardedPreviously: tier.hasOnboardedPreviously ?? false
    };
  }"""

# Patch 5: requestPost — fallback to cloudshell-gca on 403
t_requestPost = """  async requestPost(method, req, signal, retryDelay = 100) {
    const res = await this.client.request({
      url: this.getMethodUrl(method),
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...this.httpOptions.headers
      },
      responseType: "json",
      body: JSON.stringify(req),
      signal,
      retryConfig: {
        retryDelay,
        retry: 3,
        noResponseRetries: 3,
        statusCodesToRetry: [
          [429, 429],
          [499, 499],
          [500, 599]
        ]
      }
    });
    return res.data;
  }"""

r_requestPost = """  async requestPost(method, req, signal, retryDelay = 100) {
    try {
      const res = await this.client.request({
        url: this.getMethodUrl(method),
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...this.httpOptions.headers
        },
        responseType: "json",
        body: JSON.stringify(req),
        signal,
        retryConfig: {
          retryDelay,
          retry: 3,
          noResponseRetries: 3,
          statusCodesToRetry: [
            [429, 429],
            [499, 499],
            [500, 599]
          ]
        }
      });
      return res.data;
    } catch (e) {
      if (isPermissionDeniedError(e)) {
        let settingsProject = "";
        try {
          const { homedir } = await import("os");
          const { existsSync, readFileSync } = await import("fs");
          const { join } = await import("path");
          const settingsPath = join(homedir(), ".gemini", "settings.json");
          if (existsSync(settingsPath)) {
            const settings = JSON.parse(readFileSync(settingsPath, "utf8"));
            if (settings && settings.project) {
              settingsProject = settings.project;
            }
          }
        } catch (err) {}

        let fallbackProject = "";
        if (settingsProject && this.projectId !== settingsProject) {
          fallbackProject = settingsProject;
        } else if (this.projectId !== "cloudshell-gca") {
          fallbackProject = "cloudshell-gca";
        }

        if (fallbackProject) {
          if (req && req.project) req.project = fallbackProject;
          if (req && req.cloudaicompanionProject) req.cloudaicompanionProject = fallbackProject;
          this.projectId = fallbackProject;
          return this.requestPost(method, req, signal, retryDelay);
        }
      }
      throw e;
    }
  }"""

# Patch 6: requestStreamingPost — fallback to cloudshell-gca on 403
t_requestStreamingPost = """  async requestStreamingPost(method, req, signal) {
    const res = await this.client.request({
      url: this.getMethodUrl(method),
      method: "POST",
      params: {
        alt: "sse"
      },
      headers: {
        "Content-Type": "application/json",
        ...this.httpOptions.headers
      },
      responseType: "stream",
      body: JSON.stringify(req),
      signal,
      retry: false
    });
    return async function* (server) {"""

r_requestStreamingPost = """  async requestStreamingPost(method, req, signal) {
    let res;
    try {
      res = await this.client.request({
        url: this.getMethodUrl(method),
        method: "POST",
        params: {
          alt: "sse"
        },
        headers: {
          "Content-Type": "application/json",
          ...this.httpOptions.headers
        },
        responseType: "stream",
        body: JSON.stringify(req),
        signal,
        retry: false
      });
    } catch (e) {
      if (isPermissionDeniedError(e)) {
        let settingsProject = "";
        try {
          const { homedir } = await import("os");
          const { existsSync, readFileSync } = await import("fs");
          const { join } = await import("path");
          const settingsPath = join(homedir(), ".gemini", "settings.json");
          if (existsSync(settingsPath)) {
            const settings = JSON.parse(readFileSync(settingsPath, "utf8"));
            if (settings && settings.project) {
              settingsProject = settings.project;
            }
          }
        } catch (err) {}

        let fallbackProject = "";
        if (settingsProject && this.projectId !== settingsProject) {
          fallbackProject = settingsProject;
        } else if (this.projectId !== "cloudshell-gca") {
          fallbackProject = "cloudshell-gca";
        }

        if (fallbackProject) {
          if (req && req.project) req.project = fallbackProject;
          if (req && req.cloudaicompanionProject) req.cloudaicompanionProject = fallbackProject;
          this.projectId = fallbackProject;
          return this.requestStreamingPost(method, req, signal);
        }
      }
      throw e;
    }
    return async function* (server) {"""

# Patch 7: ModelDialog — filter working models and options dynamically
t_previewAccess = """getHasAccessToPreviewModel() {
  return this.hasAccessToPreviewModel ?? false;
}"""

r_previewAccess = """getHasAccessToPreviewModel() {
  return true;
}"""

t_gemini31Access = """getGemini31LaunchedSync() {
    const authType = this.contentGeneratorConfig?.authType;
    if (this.isGemini31LaunchedForAuthType(authType)) {"""

r_gemini31Access = """getGemini31LaunchedSync() { return true;
    const authType = this.contentGeneratorConfig?.authType;
    if (this.isGemini31LaunchedForAuthType(authType)) {"""

t_flashAccess = """hasGemini35FlashGAAccess() {
    const authType = this.contentGeneratorConfig?.authType;
    const hasAccess = (() => {
      if (this.isGemini31LaunchedForAuthType(authType)) {"""

r_flashAccess = """hasGemini35FlashGAAccess() { return true;
    const authType = this.contentGeneratorConfig?.authType;
    const hasAccess = (() => {
      if (this.isGemini31LaunchedForAuthType(authType)) {"""

r_getApiKeyFromEnv = """function getApiKeyFromEnv() {
  try {
    const { homedir } = require("os");
    const { existsSync, readFileSync } = require("fs");
    const { join } = require("path");
    const settingsPath = join(homedir(), ".gemini", "settings.json");
    if (existsSync(settingsPath)) {
      const settings = JSON.parse(readFileSync(settingsPath, "utf8"));
      if (settings && settings.project) {
        process.env.GOOGLE_CLOUD_PROJECT = settings.project;
      }
      if (settings?.security?.auth?.selectedType === "oauth-personal" || settings?.security?.auth?.selectedType === "compute-adc") {
        return void 0;
      }
    }
  } catch (err) {}
  const envGoogleApiKey = getEnv("GOOGLE_API_KEY");
  const envGeminiApiKey = getEnv("GEMINI_API_KEY");
  if (envGoogleApiKey && envGeminiApiKey) {
    console.warn("Both GOOGLE_API_KEY and GEMINI_API_KEY are set. Using GOOGLE_API_KEY.");
  }
  return envGoogleApiKey || envGeminiApiKey || void 0;
}"""

# Patch 13: Initialize process.env.GOOGLE_CLOUD_PROJECT on chunk load
t_chunkInit = """const require = (await import('node:module')).createRequire(import.meta.url); const __chunk_filename = (await import('node:url')).fileURLToPath(import.meta.url); const __chunk_dirname = (await import('node:path')).dirname(__chunk_filename);"""

r_chunkInit = """const require = (await import('node:module')).createRequire(import.meta.url); const __chunk_filename = (await import('node:url')).fileURLToPath(import.meta.url); const __chunk_dirname = (await import('node:path')).dirname(__chunk_filename);
try {
  const { homedir } = require("os");
  const { existsSync, readFileSync } = require("fs");
  const { join } = require("path");
  const settingsPath = join(homedir(), ".gemini", "settings.json");
  if (existsSync(settingsPath)) {
    const settings = JSON.parse(readFileSync(settingsPath, "utf8"));
    if (settings && settings.project) {
      process.env.GOOGLE_CLOUD_PROJECT = settings.project;
    }
  }
} catch (err) {}"""

# Patch 14: refreshUserQuota — fallback projectId if missing
t_refreshUserQuota = """  async refreshUserQuota() {
    const codeAssistServer = getCodeAssistServer(this);
    if (!codeAssistServer || !codeAssistServer.projectId) {
      return void 0;
    }"""

r_refreshUserQuota = """  async refreshUserQuota() {
    const codeAssistServer = getCodeAssistServer(this);
    if (codeAssistServer && !codeAssistServer.projectId) {
      let eff = "";
      try {
        const { homedir } = require("os");
        const { existsSync, readFileSync } = require("fs");
        const { join } = require("path");
        const sp = join(homedir(), ".gemini", "settings.json");
        if (existsSync(sp)) {
          const st = JSON.parse(readFileSync(sp, "utf8"));
          if (st && st.project) eff = st.project;
        }
      } catch(e) {}
      codeAssistServer.projectId = eff || process.env.GOOGLE_CLOUD_PROJECT || "cloudshell-gca";
    }
    if (!codeAssistServer) {
      return void 0;
    }"""

patches = [
    (t_loadCodeAssist_try, r_loadCodeAssist_try),
    (t_loadCodeAssist_catch, r_loadCodeAssist_catch),
    (t_listExperiments, r_listExperiments),
    (t_setupUser1, r_setupUser1),
    (t_setupUser2, r_setupUser2),
    (t_previewAccess, r_previewAccess),
    (t_gemini31Access, r_gemini31Access),
    (t_flashAccess, r_flashAccess),
]

patched_files = []
if os.path.exists(bundle_dir):
    for fname in os.listdir(bundle_dir):
        if not fname.endswith(".js"):
            continue
        fpath = os.path.join(bundle_dir, fname)
        try:
            with open(fpath, "r", encoding="utf-8") as f:
                content = f.read()
        except Exception:
            continue

        changed = False
        content_norm = content.replace("\r\n", "\n")
        for target, replacement in patches:
            target_norm = target.replace("\r\n", "\n")
            replacement_norm = replacement.replace("\r\n", "\n")
            if target_norm in content_norm:
                content_norm = content_norm.replace(target_norm, replacement_norm)
                changed = True

        if changed:
            # Write back with Unix newlines (or keep the normalized content)
            with open(fpath, "w", encoding="utf-8", newline="\n") as f:
                f.write(content_norm)
            patched_files.append(fname)

print(f"OK: patched {len(patched_files)} files")
