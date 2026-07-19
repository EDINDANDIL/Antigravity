use sha2::{Sha256, Digest};
use std::env;
use std::fs::{self, File};
use std::io::{self, Write, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use regex::Regex;
use std::thread;
use std::time::Duration;

fn clear_screen() {
    if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "cls"]).status().unwrap();
    } else {
        print!("{}[2J{}[1;1H", 27 as char, 27 as char);
    }
}

fn verify_key(key: &str) -> bool {
    let k: String = key.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let k = k.to_uppercase();
    if k.len() != 24 { return false; }
    
    let mut hasher = Sha256::new();
    hasher.update(&k[..12]);
    obfstr::obfstr! {
        let secret = "___LICENSE_SECRET___";
    }
    hasher.update(secret.as_bytes());
    let expected = hex::encode(hasher.finalize()).to_uppercase();
    let expected = &expected[..12];
    
    if k[12..].len() != expected.len() { return false; }
    
    let mut result = 0;
    for (x, y) in k[12..].chars().zip(expected.chars()) {
        result |= (x as u8) ^ (y as u8);
    }
    thread::sleep(Duration::from_millis(300));
    result == 0
}

fn login_screen() {
    loop {
        clear_screen();
        println!("{}", "=== ПРОВЕРКА ДОСТУПА ===");
        println!();
        print!("{}", "Введите лицензионный ключ: ");
        io::stdout().flush().unwrap();

        let mut key = String::new();
        let bytes_read = io::stdin().read_line(&mut key).unwrap_or(0);

        if bytes_read == 0 {
            std::process::exit(1);
        }

        let k = key.trim().replace("\"", "");

        if verify_key(&k) {
            println!("{}", "Доступ разрешён.");
            thread::sleep(Duration::from_secs(1));
            return;
        } else {
            println!("{}", "Доступ запрещён. Неверный ключ.");
            thread::sleep(Duration::from_secs(2));
        }
    }
}

fn find_all_installs() -> Vec<PathBuf> {
    let mut installs = Vec::new();
    let local_appdata = env::var("LOCALAPPDATA").unwrap_or_default();
    let prog_files = env::var("PROGRAMFILES").unwrap_or_default();
    let prog_files_x86 = env::var("PROGRAMFILES(X86)").unwrap_or_default();
    
    let candidates = vec![
        PathBuf::from(&local_appdata).join("Programs").join("Antigravity"),
        PathBuf::from(&prog_files).join("Antigravity"),
        PathBuf::from(&prog_files_x86).join("Antigravity"),
        PathBuf::from(&local_appdata).join("Antigravity"),
        PathBuf::from(&local_appdata).join("Programs").join("Antigravity IDE"),
        PathBuf::from(&prog_files).join("Antigravity IDE"),
        PathBuf::from(&prog_files_x86).join("Antigravity IDE"),
        PathBuf::from(&local_appdata).join("Antigravity IDE"),
        PathBuf::from("D:\\Programs\\Antigravity IDE"),
        PathBuf::from("C:\\Programs\\Antigravity IDE"),
        PathBuf::from(&local_appdata).join("agy").join("bin")
    ];
    
    for path in candidates {
        if path.exists() && path.is_dir() {
            if path.join("resources").exists() && !installs.contains(&path) {
                installs.push(path.clone());
            } else if path.join("agy.exe").exists() && !installs.contains(&path) {
                installs.push(path.clone());
            }
        }
    }
    installs
}

fn patch_binary(_inst: &Path, bin_path: &Path) -> Result<(), String> {
    let mut data = fs::read(bin_path).map_err(|e| e.to_string())?;
    
    obfstr::obfstr! {
        let old_str = "ineligible";
        let new_str = "inexigible";
    }
    let old_bytes = old_str.as_bytes();
    let new_bytes = new_str.as_bytes();
    
    let mut found = false;
    for i in 0..data.len() - old_bytes.len() {
        if &data[i..i+old_bytes.len()] == old_bytes {
            data[i..i+new_bytes.len()].copy_from_slice(new_bytes);
            found = true;
        }
    }
    
    if found {
        let file_name = bin_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        Command::new("taskkill").args(&["/F", "/IM", &file_name]).output().ok();
        thread::sleep(Duration::from_millis(500));

        fs::write(bin_path, data).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        let mut already_patched = false;
        for i in 0..data.len() - new_bytes.len() {
            if &data[i..i+new_bytes.len()] == new_bytes {
                already_patched = true;
                break;
            }
        }
        if already_patched {
            Ok(())
        } else {
            Err("Сигнатура не найдена".to_string())
        }
    }
}

// Roll back a previous binary patch (`inexigible` -> `ineligible`) in-place. The 8 hits
// in language_server live inside protobuf descriptors / reflection tags — touching
// them is unnecessary (the frontend / main.js patch already handles eligibility) and
// carries the risk of corrupting gRPC marshalling. If a prior version of this tool
// already patched the LS, undo it.
#[allow(dead_code)]
fn unpatch_binary(bin_path: &Path) -> Result<bool, String> {
    let mut data = fs::read(bin_path).map_err(|e| e.to_string())?;
    let needle = b"inexigible";
    let replacement = b"ineligible";

    let mut found = false;
    if data.len() >= needle.len() {
        for i in 0..=data.len() - needle.len() {
            if &data[i..i + needle.len()] == needle {
                data[i..i + replacement.len()].copy_from_slice(replacement);
                found = true;
            }
        }
    }

    if found {
        let file_name = bin_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        Command::new("taskkill").args(&["/F", "/IM", &file_name]).output().ok();
        thread::sleep(Duration::from_millis(500));
        fs::write(bin_path, data).map_err(|e| e.to_string())?;
    }
    Ok(found)
}


fn kill_affected_processes() {
    println!("\x1b[93m[INFO] Завершаем запущенные процессы перед патчингом...\x1b[0m");
    let processes = [
        "Antigravity.exe",
        "Antigravity CLI.exe",
        "agy.exe",
        "language_server.exe",
        "language_server_windows_x64.exe",
        "node.exe"
    ];
    for p in processes.iter() {
        Command::new("taskkill")
            .args(&["/F", "/IM", p])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok();
    }
    thread::sleep(Duration::from_millis(1000));
}

fn patch_all_binaries(inst: &Path) {
    // CLI: agy.exe carries a user-facing eligibility check that we want to bypass.
    let cli = inst.join("agy.exe");
    if cli.exists() {
        match patch_binary(inst, &cli) {
            Ok(_) => println!("  [OK] CLI binary patched"),
            Err(e) => println!("  [ERR] CLI patch failed: {}", e),
        }
    }

    // LS binaries (language_server.exe): patch `ineligible` -> `inexigible` so the
    // renderer (served by the LS) never sees `"ineligible": true` in JSON API responses.
    // This affects protobuf JSON marshalling — field names change but field numbers
    // (used in proto binary encoding) remain intact.
    let ls_candidates = [
        inst.join("resources").join("bin").join("language_server.exe"),
        inst.join("resources").join("app").join("extensions").join("antigravity").join("bin").join("language_server_windows_x64.exe"),
        inst.join("resources").join("app").join("extensions").join("antigravity").join("bin").join("language_server.exe"),
    ];
    for ls in ls_candidates.iter() {
        if ls.exists() {
            match patch_binary(inst, ls) {
                Ok(_) => println!("  [OK] LS binary patched"),
                Err(e) => println!("  [ERR] LS patch failed: {}", e),
            }
        }
    }
}

fn extract_asar(app_asar: &Path, app_dir: &Path) -> bool {
    if !app_asar.exists() || app_dir.exists() { return true; }
    
    let mut file = match File::open(app_asar) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut header_size_bytes = [0u8; 16];
    if file.read_exact(&mut header_size_bytes).is_err() {
        return false;
    }

    let _data_size = u32::from_le_bytes([header_size_bytes[0], header_size_bytes[1], header_size_bytes[2], header_size_bytes[3]]);
    let header_size = u32::from_le_bytes([header_size_bytes[4], header_size_bytes[5], header_size_bytes[6], header_size_bytes[7]]);
    let _header_object_size = u32::from_le_bytes([header_size_bytes[8], header_size_bytes[9], header_size_bytes[10], header_size_bytes[11]]);
    let header_string_size = u32::from_le_bytes([header_size_bytes[12], header_size_bytes[13], header_size_bytes[14], header_size_bytes[15]]);

    let mut header_bytes = vec![0u8; header_string_size as usize];
    if file.read_exact(&mut header_bytes).is_err() {
        return false;
    }

    let header_json = match String::from_utf8(header_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let tree: serde_json::Value = match serde_json::from_str(&header_json) {
        Ok(t) => t,
        Err(_) => return false,
    };

    let base_offset = (header_size + 8) as u64;

    fn extract_entry(
        file: &mut File,
        base_offset: u64,
        entry: &serde_json::Value,
        current_path: &Path,
    ) -> Result<(), std::io::Error> {
        if let Some(files) = entry.get("files") {
            if let Some(obj) = files.as_object() {
                for (name, child) in obj {
                    let next_path = current_path.join(name);
                    extract_entry(file, base_offset, child, &next_path)?;
                }
            }
        } else {
            let size = entry.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
            let offset_str = entry.get("offset").and_then(|o| o.as_str()).unwrap_or("0");
            let offset = offset_str.parse::<u64>().unwrap_or(0);
            let unpacked = entry.get("unpacked").and_then(|u| u.as_bool()).unwrap_or(false);

            if let Some(parent) = current_path.parent() {
                fs::create_dir_all(parent)?;
            }

            if !unpacked {
                file.seek(SeekFrom::Start(base_offset + offset))?;
                let mut out_file = File::create(current_path)?;
                let mut remaining = size;
                let mut buffer = [0u8; 65536];
                while remaining > 0 {
                    let to_read = std::cmp::min(remaining, buffer.len() as u64) as usize;
                    file.read_exact(&mut buffer[..to_read])?;
                    out_file.write_all(&buffer[..to_read])?;
                    remaining -= to_read as u64;
                }
            }
        }
        Ok(())
    }

    let asar_unpacked_path = app_asar.with_extension("asar.unpacked");
    
    if extract_entry(&mut file, base_offset, &tree, app_dir).is_err() {
        return false;
    }

    if asar_unpacked_path.exists() && asar_unpacked_path.is_dir() {
        fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
            fs::create_dir_all(dst)?;
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                let file_type = entry.file_type()?;
                let dest_path = dst.join(entry.file_name());
                if file_type.is_dir() {
                    copy_dir_all(&entry.path(), &dest_path)?;
                } else {
                    fs::copy(&entry.path(), &dest_path)?;
                }
            }
            Ok(())
        }
        let _ = copy_dir_all(&asar_unpacked_path, app_dir);
    }

    let bak = app_asar.with_extension("asar.bak");
    if fs::rename(app_asar, bak).is_err() {
        return false;
    }

    true
}

fn detect_stacked_ide(content: &str) -> bool {
    // Markers from Python patcher or from a previous failed/duplicate run
    content.contains("/*[AG_PATCHED]*/") || content.contains("[AG_PROXY_HOOK]")
}

fn patch_ide(_inst: &Path, main_js: &Path) -> Result<(), String> {
    let content = fs::read_to_string(main_js).map_err(|e| e.to_string())?;

    // Already correctly patched by our current version — skip.
    if content.trim_end().ends_with("// UNLOCKED") && !detect_stacked_ide(&content) {
        return Ok(());
    }

    // Detected an older / foreign patch.
    if detect_stacked_ide(&content) || content.trim_end().ends_with("// UNLOCKED") {
        return Err("Обнаружена старая версия патча. Пожалуйста, выполните чистую переустановку Antigravity IDE перед повторным патчем.".into());
    }

    obfstr::obfstr! {
        let pattern_str = r#"async\s+([A-Za-z_$0-9]+)\(([A-Za-z_$0-9]+)\)\s*\{\s*if\(this\.([A-Za-z_$0-9]+)\.send\(\{type:[A-Za-z_$0-9]+\.isGcpTos\?"GCP_SIGN_IN":"SIGN_IN"\}\),this\.([A-Za-z_$0-9]+)\.resetIsTierGCPTos\(\),this\.[A-Za-z_$0-9]+\.isGoogleInternal\)\{try\{await this\.([A-Za-z_$0-9]+)\.loadCodeAssist\([A-Za-z_$0-9]+\);const\{settings:([A-Za-z_$0-9]+),userTier:([A-Za-z_$0-9]+)\}=await this\.refreshUserStatus\([A-Za-z_$0-9]+\),([A-Za-z_$0-9]+)=([A-Za-z_$0-9]+)\([A-Za-z_$0-9]+\);this\.([A-Za-z_$0-9]+)\.pushUpdate\([A-Za-z_$0-9]+\),this\.[A-Za-z_$0-9]+\.send\(\{type:"AUTH_SUCCESS",tokenInfo:[A-Za-z_$0-9]+\}\),this\.([A-Za-z_$0-9]+)\.fire\(\{settings:[A-Za-z_$0-9]+,userTier:[A-Za-z_$0-9]+\}\)\}catch\(([A-Za-z_$0-9]+)\)\{.*?(?:return\}|return;\s*\})"#;
    }

    let re = Regex::new(pattern_str).unwrap();
    if let Some(caps) = re.captures(&content) {
        let fname = caps.get(1).unwrap().as_str();
        let var_t = caps.get(2).unwrap().as_str();
        let var_t_send = caps.get(3).unwrap().as_str();
        let var_y = caps.get(4).unwrap().as_str();
        let var_func = caps.get(9).unwrap().as_str();
        let var_f = caps.get(10).unwrap().as_str();
        let var_h = caps.get(11).unwrap().as_str();
        let var_i = caps.get(8).unwrap().as_str();

        let payload = format!(r#"async {fname}({var_t}){{
    this.{var_t_send}.send({{type:{var_t}.isGcpTos?"GCP_SIGN_IN":"SIGN_IN"}});
    this.{var_y}.resetIsTierGCPTos();
    try {{
        try {{ await this.{var_y}.loadCodeAssist({var_t}); }} catch(_) {{}}
        try {{ await this.{var_y}.onboardUser("standard-tier", {var_t}); }} catch(_) {{
            try {{ await this.{var_y}.onboardUser("free-tier", {var_t}); }} catch(__) {{}}
        }}
        let __res = {{ settings: {{}}, userTier: {{ id: "pro", description: "Pro" }} }};
        try {{ __res = await this.refreshUserStatus({var_t}); }} catch(_) {{}}
        const {var_i} = {var_func}({var_t});
        try {{ this.{var_f}.pushUpdate({var_i}); }} catch(_) {{}}
        this.{var_t_send}.send({{type:"AUTH_SUCCESS",tokenInfo:{var_t}}});
        this.{var_h}.fire({{settings:__res.settings, userTier:__res.userTier}});
    }} catch(e) {{}}
    return;
"#);

        let new_content = format!("{}\n// UNLOCKED", content[..caps.get(0).unwrap().start()].to_string() + &payload + &content[caps.get(0).unwrap().end()..]);
        fs::write(main_js, new_content).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Сигнатура не найдена (возможно, установлена другая версия)".to_string())
    }
}

fn patch_desktop(_inst: &Path, main_js: &Path) -> Result<(), String> {
    let content = fs::read_to_string(main_js).map_err(|e| e.to_string())?;

    // 1. Strip any old hook section entirely so we never stack
    let cleaned = strip_desktop_hook(&content);

    // 2. Apply inline bypasses directly to main.js so even if the proxy misses
    //    a request, the local code never sees ineligible.
    let inline_patched = apply_desktop_inline_patches(&cleaned);

    obfstr::obfstr! {
        let hook = r#"// [AG_PROXY_HOOK]
const _elec = require('electron');
const http = require('http');
const https = require('https');
const zlib = require('zlib');

let agProxyPort = 0;
let realLsPort = 0;
let agApiPort = 0;

const proxyServer = http.createServer((req, res) => {
    if (!realLsPort) {
        if (!res.headersSent) res.writeHead(500);
        return res.end('No LS port');
    }
    const options = {
        hostname: '127.0.0.1',
        port: realLsPort,
        path: req.url,
        method: req.method,
        headers: req.headers,
        rejectUnauthorized: false
    };

    const proxyReq = https.request(options, (proxyRes) => {
        if (req.url.includes('main.js')) {
            const chunks = [];
            proxyRes.on('data', c => chunks.push(c));
            proxyRes.on('end', () => {
                let buffer = Buffer.concat(chunks);
                const encoding = (proxyRes.headers['content-encoding'] || '').toLowerCase();
                let wasCompressed = false;
                if (encoding.includes('gzip')) {
                    try { buffer = zlib.gunzipSync(buffer); wasCompressed = true; } catch(e){}
                } else if (encoding.includes('br')) {
                    try { buffer = zlib.brotliDecompressSync(buffer); wasCompressed = true; } catch(e){}
                }

                let body = buffer.toString('utf-8');

                body = body.replace(/_handleAuthErrorResponse\(([a-zA-Z_$]+)\)\{var ([a-zA-Z_$]+)=\1\?\.failureDetails;/g, '_handleAuthErrorResponse($1){var $2=$1?.failureDetails; if($2?.case==="ineligible"){ this._authActor.send({type:"AUTH_SUCCESS",tokenInfo:{accessToken:""},scopes:[],isGcpTos:false}); return; }');
                body = body.replace(/\?\.failureDetails\?\.case==="ineligible"\?this\._authActor\.send\(\{type:"SET_INELIGIBLE"/g, '?.failureDetails?.case==="NEVER_MATCH"?this._authActor.send({type:"SET_INELIGIBLE"');

                const pattern2 = /let ([A-Za-z_$]+)=.*\.getUserStatus\(\{\}\)\)\)\.userStatus;if\(\1\)\{/g;
                body = body.replace(pattern2, (match, p1) => {
                    return match.replace(`if(${p1}){`, `${p1}={planStatus:{planInfo:{planName:"pro"}}, disableTelemetry:false, userDataCollectionForceDisabled:false};if(${p1}){`);
                });

                let outBuffer = Buffer.from(body, 'utf-8');
                if (wasCompressed) {
                    if (encoding.includes('gzip')) outBuffer = zlib.gzipSync(outBuffer);
                    else if (encoding.includes('br')) outBuffer = zlib.brotliCompressSync(outBuffer);
                }

                const headers = { ...proxyRes.headers };
                headers['content-length'] = outBuffer.length;
                if (!res.headersSent) res.writeHead(proxyRes.statusCode, headers);
                res.end(outBuffer);
            });
        } else {
            if (!res.headersSent) res.writeHead(proxyRes.statusCode, proxyRes.headers);
            proxyRes.pipe(res, { end: true });
        }
    });

    proxyReq.on('error', (e) => {
        if (!res.headersSent) res.writeHead(500);
        res.end();
    });
    req.pipe(proxyReq, { end: true });
});

proxyServer.listen(0, '127.0.0.1', () => { agProxyPort = proxyServer.address().port; });

const apiProxyServer = http.createServer((req, res) => {
    res.setHeader('Content-Type', 'application/json');
    res.setHeader('Access-Control-Allow-Origin', '*');
    const body = [];
    req.on('data', c => body.push(c));
    req.on('end', () => {
        const reqBody = Buffer.concat(body).toString('utf-8');
        if (req.url.includes('UserStatus') || req.url.includes('getUserStatus') || req.url.includes('refreshUserStatus') || req.url.includes('userStatus')) {
            return res.end(JSON.stringify({
                userStatus: {
                    userTier: { id: "pro", description: "Pro" },
                    currentTier: { id: "STANDARD", hasOnboardedPreviously: true },
                    planStatus: { planInfo: { planName: "pro", isEligible: true } },
                    eligible: true,
                    ineligibleReason: null,
                    disableTelemetry: false,
                    userDataCollectionForceDisabled: false,
                    allowedTiers: [{ id: "STANDARD", name: "Standard", isDefault: true }]
                }
            }));
        }
        if (req.url.includes('loadCodeAssist')) {
            return res.end(JSON.stringify({
                currentTier: { id: "STANDARD", hasOnboardedPreviously: true },
                cloudaicompanionProject: "",
                allowedTiers: [{ id: "STANDARD", name: "Standard", isDefault: true }],
                paidTier: void 0
            }));
        }
        if (req.url.includes('onboardUser')) {
            return res.end(JSON.stringify({
                done: true,
                response: {
                    cloudaicompanionProject: { id: "" }
                }
            }));
        }
        if (req.url.includes('listExperiments')) {
            return res.end(JSON.stringify({ flags: [], experimentIds: [] }));
        }
        if (req.url.includes('fetchAdminControls')) {
            return res.end(JSON.stringify({}));
        }
        if (req.url.includes('getCodeAssistGlobalUserSetting')) {
            return res.end(JSON.stringify({}));
        }
        if (req.url.includes('refreshUserQuota') || req.url.includes('retrieveUserQuota')) {
            return res.end(JSON.stringify({}));
        }
        if (req.url.includes('listAvailableTiers') || req.url.includes('isEligible')) {
            return res.end(JSON.stringify({ tiers: [], isEligible: true, ineligibleReason: null }));
        }
        // Catch-all: return eligible response for any unrecognized endpoint
        // so the Desktop never sees an empty response (which it might interpret as ineligible).
        return res.end(JSON.stringify({
            eligible: true,
            ineligibleReason: null,
            allowedTiers: [{ id: "STANDARD", name: "Standard", isDefault: true }],
            currentTier: { id: "STANDARD", hasOnboardedPreviously: true },
            planStatus: { planInfo: { planName: "pro", isEligible: true } },
            userTier: { id: "pro", description: "Pro" }
        }));
    });
});

apiProxyServer.listen(0, '127.0.0.1', () => { agApiPort = apiProxyServer.address().port; });

const _origWhenReady = _elec.app.whenReady;
_elec.app.whenReady = function() {
    return _origWhenReady.call(this).then(() => {
        _elec.session.defaultSession.webRequest.onBeforeRequest({ urls: ['*://*.googleapis.com/*', '*://127.0.0.1:*/*', '*://localhost:*/*'] }, (details, callback) => {
            const urlObj = new URL(details.url);
            if (urlObj.hostname.includes('googleapis.com')) {
                return callback({ redirectURL: `http://127.0.0.1:${agApiPort}${urlObj.pathname}` });
            }
            if (urlObj.port != agProxyPort && !details.url.includes('ag_bypass')) {
                realLsPort = urlObj.port;
                if (details.url.includes('.js') || details.url.includes('.css') || details.url.includes('.png') || details.url.includes('.woff') || details.url.includes('main.js') || details.url.includes('index.html') || details.url.includes('/')) {
                    urlObj.protocol = 'http:'; urlObj.port = agProxyPort; urlObj.searchParams.set('ag_bypass', '1');
                    return callback({ redirectURL: urlObj.toString() });
                }
            }
            callback({});
        });
    });
};
// [/AG_PROXY_HOOK]
"#;
    }

    let new_content = format!("{}\n{}\n// UNLOCKED", hook, inline_patched);
    fs::write(main_js, new_content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove any existing `// [AG_PROXY_HOOK]` … `// [/AG_PROXY_HOOK]` section
/// so we never stack multiple hooks.
fn strip_desktop_hook(content: &str) -> String {
    if let Some(start) = content.find("// [AG_PROXY_HOOK]") {
        if let Some(end) = content[start..].find("// [/AG_PROXY_HOOK]") {
            let after = start + end + "// [/AG_PROXY_HOOK]".len();
            // Also strip the trailing `// UNLOCKED` that followed the hook
            let mut rest = content[after..].to_string();
            if rest.trim_end().ends_with("// UNLOCKED") {
                if let Some(pos) = rest.rfind("// UNLOCKED") {
                    rest = rest[..pos].to_string();
                }
            }
            return content[..start].to_string() + &rest;
        }
    }
    // Also strip a bare `// UNLOCKED` if there's no hook (from a previous test run)
    let trimmed = content.trim_end();
    if trimmed.ends_with("// UNLOCKED") {
        if let Some(pos) = content.rfind("// UNLOCKED") {
            return content[..pos].to_string() + "\n";
        }
    }
    content.to_string()
}

/// Apply inline bypass patterns directly to main.js so that
/// eligibility checks always pass regardless of proxy interception.
fn apply_desktop_inline_patches(content: &str) -> String {
    let mut output = content.to_string();

    // 0) Nuclear option: rename "ineligible" to "inexigible" everywhere in JS
    //    (same strategy as the agy.exe binary patch). Catches ANY code path
    //    that checks for ineligibility, regardless of format.
    output = output.replace("ineligible", "inexigible");

    // 1. getUserStatus inline injection
    //    Pattern: let X=...getUserStatus({}))).userStatus;if(X){
    //    Rust regex doesn't support backreferences, so we capture both identifier
    //    positions separately and only replace when they match.
    let re_getus = Regex::new(r#"let ([A-Za-z_$]+)=.*\.getUserStatus\(\{\}\)\)\)\.userStatus;if([A-Za-z_$]+)\{"#).unwrap();
    output = re_getus.replace_all(&output, |caps: &regex::Captures| {
        if caps.get(1).map_or("", |m| m.as_str()) != caps.get(2).map_or("", |m| m.as_str()) {
            return caps.get(0).map_or("", |m| m.as_str()).to_string();
        }
        let v = caps.get(1).map_or("", |m| m.as_str());
        format!("let {v}=...getUserStatus({{}}))).userStatus;{v}={{\"planStatus\":{{\"planInfo\":{{\"planName\":\"pro\"}}}},\"disableTelemetry\":false,\"userDataCollectionForceDisabled\":false}};if({v}){{")
    }).to_string();

    // 2. _handleAuthErrorResponse ineligible -> AUTH_SUCCESS
    //    Pattern: _handleAuthErrorResponse(X){var Y=X?.failureDetails;
    let re_auth = Regex::new(r#"_handleAuthErrorResponse\(([A-Za-z_$]+)\)\{var ([A-Za-z_$]+)=([A-Za-z_$]+)\?\.failureDetails;"#).unwrap();
    output = re_auth.replace_all(&output, |caps: &regex::Captures| {
        let p = caps.get(1).map_or("", |m| m.as_str());  // X (parameter)
        let v = caps.get(2).map_or("", |m| m.as_str());  // Y (local var)
        let s = caps.get(3).map_or("", |m| m.as_str());  // should be same as X
        if p != s {
            return caps.get(0).map_or("", |m| m.as_str()).to_string();
        }
        format!("_handleAuthErrorResponse({p}){{var {v}={p}?.failureDetails; if({v}?.case===\"inexigible\"){{ this._authActor.send({{type:\"AUTH_SUCCESS\",tokenInfo:{{accessToken:\"\"}},scopes:[],isGcpTos:false}}); return; }}")
    }).to_string();

    // 3. SET_INELIGIBLE -> NEVER_MATCH (disable ineligible state transition)
    let re_inel = Regex::new(r#"\?\.failureDetails\?\.case===\"ineligible\"\?this\._authActor\.send\(\{type:\"SET_INELIGIBLE\""#).unwrap();
    output = re_inel.replace_all(&output, |_caps: &regex::Captures| {
        "?.failureDetails?.case===\"NEVER_MATCH\"?this._authActor.send({type:\"SET_INELIGIBLE\""
    }).to_string();

    // 4. Onboard eligibility: replace `isEligible` response checks to always pass
    let re_eligible = Regex::new(r#"(isEligible:\s*)false"#).unwrap();
    output = re_eligible.replace_all(&output, |caps: &regex::Captures| {
        format!("{}true", &caps[1])
    }).to_string();

    output
}

fn patch_extension_js(inst: &Path) -> Result<bool, String> {
    let ext_path = inst.join("resources").join("app").join("extensions")
        .join("antigravity").join("dist").join("extension.js");
    if !ext_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&ext_path).map_err(|e| e.to_string())?;
    if content.contains("/*[AG_EXT_PATCHED]*/") {
        return Ok(true);
    }

    obfstr::obfstr! {
        // Match `if(!t)return[];` that lives between `getUserStatus()` and the `(0,X)(t,Y)` parse.
        // The Rust regex crate doesn't support backreferences, so the `===email?` term uses an
        // independent capture (group 6); in real minified output groups 4 and 6 hold the same
        // identifier, and we drop group 6 in the replacement.
        let p1_str = r#"const t=await ([A-Za-z_$][A-Za-z_$0-9.]*)\.UserStatus\.getUserStatus\(\);if\(!t\)return\[\];const n=\(0,([A-Za-z_$][A-Za-z_$0-9.]*)\)\(t,([A-Za-z_$][A-Za-z_$0-9.]*)\),\{email:([A-Za-z_$][A-Za-z_$0-9]*),name:([A-Za-z_$][A-Za-z_$0-9]*)\}=n;return""===([A-Za-z_$][A-Za-z_$0-9]*)\?\[\]:"#;
    }
    let p1 = Regex::new(p1_str).unwrap();
    let new_content = p1.replace(&content, |caps: &regex::Captures| {
        let ns = &caps[1];
        let p2 = &caps[2];
        let dz7 = &caps[3];
        let email = &caps[4];
        let name = &caps[5];
        format!(
            "const t=await {ns}.UserStatus.getUserStatus();let {email}=\"\",{name}=\"\";try{{if(t){{const n=(0,{p2})(t,{dz7});{email}=n.email||\"\";{name}=n.name||\"\";}}}}catch(_){{}}if({email}===\"\"){{{email}=\"antigravity-user\";{name}=\"User\";}}return false?[]:"
        )
    });

    if new_content == content {
        return Err("Сигнатура extension.js не найдена (возможно, другая версия)".to_string());
    }

    let marked = format!("/*[AG_EXT_PATCHED]*/\n{}", new_content);
    fs::write(&ext_path, marked).map_err(|e| e.to_string())?;
    Ok(true)
}

fn process_install(install: &Path) -> Result<String, String> {
    // Kill any running Antigravity processes to avoid file locks during patching.
    for proc_name in &["antigravity", "language_server", "agy"] {
        Command::new("taskkill")
            .args(&["/F", "/IM", &format!("{}.exe", proc_name)])
            .output()
            .ok();
    }
    thread::sleep(Duration::from_millis(1000));

    // Patch all relevant binaries (Language Server / CLI)
    patch_all_binaries(install);

    if install.join("agy.exe").exists() {
        return Ok("Antigravity CLI".to_string());
    }

    let resources = install.join("resources");
    let app_dir = resources.join("app");
    let app_asar = resources.join("app.asar");

    if app_asar.exists() {
        // Always re-extract to avoid stale files from a previous patch run.
        if app_dir.exists() {
            let _ = fs::remove_dir_all(&app_dir);
        }
        if !extract_asar(&app_asar, &app_dir) {
            return Err("Ошибка получения доступа к приложению".to_string());
        }
    }

    let ide_js = app_dir.join("out").join("main.js");
    let desktop_js = app_dir.join("dist").join("main.js");

    if ide_js.exists() {
        patch_ide(install, &ide_js)?;
        // Best-effort: extension patch failure should not break the IDE patch result,
        // but is reported as a warning to the user.
        if let Err(e) = patch_extension_js(install) {
            println!("{} {}", "[WARN] Патч extension.js пропущен:", e);
        }
        return Ok("Antigravity IDE".to_string());
    } else if desktop_js.exists() {
        patch_desktop(install, &desktop_js)?;
        return Ok("Antigravity Desktop".to_string());
    }

    Err("Компоненты приложения не найдены".to_string())
}

// NRPT-based selective DNS routing: only Google API namespaces are sent to
// xbox-dns.ru servers; everything else stays on the system default. Tagged via
// Comment so we can find and remove our rules later without touching others.
const AG_NRPT_TAG: &str = "AG_UNLOCKER_NRPT";
const AG_NRPT_NAMESPACES: &[&str] = &[
    ".googleapis.com",
    ".googleusercontent.com",
    "accounts.google.com",
    ".google",
    ".goog",
];
// xbox-dns.ru servers (v4 + v6); Windows will pick whichever family is reachable.
const AG_NRPT_NAMESERVERS: &str = "'111.88.96.50','111.88.96.51','2a00:ab00:1233:26::50','2a00:ab00:1233:26::51'";

fn remove_dns_nrpt() {
    // Restore default IPv6 prefix policy (IPv6 preferred over IPv4)
    Command::new("netsh")
        .args(["interface", "ipv6", "set", "prefixpolicy", "::ffff:0:0/96", "35", "4"])
        .output()
        .ok();

    let cmd = format!(
        "Get-DnsClientNrptRule -ErrorAction SilentlyContinue | Where-Object {{ $_.Comment -eq '{}' }} | Remove-DnsClientNrptRule -Force -ErrorAction SilentlyContinue; Clear-DnsClientCache -ErrorAction SilentlyContinue",
        AG_NRPT_TAG
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
        .output()
        .ok();
}

fn is_nrpt_applied() -> bool {
    let cmd = format!(
        "(Get-DnsClientNrptRule -ErrorAction SilentlyContinue | Where-Object {{$_.Comment -eq '{}'}} | Measure-Object).Count",
        AG_NRPT_TAG
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
        .output();
    match out {
        Ok(o) => {
            let count_str = String::from_utf8_lossy(&o.stdout).trim().to_string();
            count_str.parse::<usize>().unwrap_or(0) >= AG_NRPT_NAMESPACES.len()
        }
        Err(_) => false,
    }
}

fn setup_dns_nrpt() -> Result<(), String> {
    // Remove any of our previous rules to keep a clean idempotent state.
    remove_dns_nrpt();

    // Prefer IPv4 over IPv6 globally to prevent proxy connection drops/hangs on unreachable IPv6
    Command::new("netsh")
        .args(["interface", "ipv6", "set", "prefixpolicy", "::ffff:0:0/96", "46", "4"])
        .output()
        .ok();

    for namespace in AG_NRPT_NAMESPACES {
        let cmd = format!(
            "Add-DnsClientNrptRule -Namespace '{}' -NameServers @({}) -Comment '{}' -DisplayName 'AG Unlocker' -ErrorAction Stop",
            namespace, AG_NRPT_NAMESERVERS, AG_NRPT_TAG
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let hint = if stderr.contains("Access is denied")
                || stderr.contains("requires elevation")
                || stderr.contains("Access denied")
                || stderr.contains("denied")
            {
                "требуются права администратора".to_string()
            } else if stderr.is_empty() {
                "PowerShell завершился с ошибкой".to_string()
            } else {
                stderr
            };
            return Err(format!("NRPT для {}: {}", namespace, hint));
        }
    }
    // Flush so the new policy takes effect immediately without a reboot.
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "Clear-DnsClientCache -ErrorAction SilentlyContinue"])
        .output()
        .ok();
    Ok(())
}

fn is_gemini_cli_installed() -> bool {
    if let Ok(appdata) = env::var("APPDATA") {
        let path = PathBuf::from(appdata)
            .join("npm")
            .join("node_modules")
            .join("@google")
            .join("gemini-cli");
        path.exists() && path.is_dir()
    } else {
        false
    }
}

fn handle_restore_dns() {
    print!("Удаление NRPT-правил DNS... ");
    io::stdout().flush().ok();
    remove_dns_nrpt();
    println!("готово.");

    println!("{}", "Готово!");
    thread::sleep(Duration::from_secs(2));
}

fn mask_path(path: &str) -> String {
    let mut result = path.to_string();
    if let Ok(local) = env::var("LOCALAPPDATA") {
        result = result.replace(&local, "%LOCALAPPDATA%");
    }
    if let Ok(appdata) = env::var("APPDATA") {
        result = result.replace(&appdata, "%APPDATA%");
    }
    if let Ok(userprofile) = env::var("USERPROFILE") {
        result = result.replace(&userprofile, "%USERPROFILE%");
    }
    result
}



fn run_gemini_patcher() -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let patch_script_path = temp_dir.join("patch_gemini.py");
    
    let script_content = include_str!("patch_gemini.py");
    if let Err(e) = std::fs::write(&patch_script_path, script_content) {
        return Err(format!("Не удалось записать скрипт во временную директорию: {}", e));
    }
    
    let out = std::process::Command::new("python")
        .args([patch_script_path.to_str().unwrap()])
        .output()
        .map_err(|e| format!("Не удалось запустить python: {}", e))?;
        
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("Скрипт завершился с ошибкой: {}", stderr));
    }
    
    Ok(())
}

fn is_valid_gemini_api_key(key: &str) -> bool {
    key.trim().starts_with("AIzaSy") && key.trim().len() == 39
}

fn get_system_gcloud_project() -> Option<String> {
    // Check env var first
    if let Ok(proj) = env::var("GOOGLE_CLOUD_PROJECT") {
        if !proj.is_empty() { return Some(proj.trim().to_string()); }
    }
    // Check Windows env var
    #[cfg(target_os = "windows")]
    {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", "[Environment]::GetEnvironmentVariable('GOOGLE_CLOUD_PROJECT', 'User')"])
            .output()
            .ok()?;
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !stdout.is_empty() { return Some(stdout); }
        }
    }
    // Check Gemini CLI settings.json
    let settings_path = format!(
        "{}\\.gemini\\settings.json",
        env::var("USERPROFILE").unwrap_or_default()
    );
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
        // Look for "project":"..." in JSON
        if let Some(start) = content.find(r#""project":""#) {
            let remainder = &content[start + 11..];
            if let Some(end) = remainder.find('"') {
                let proj = &remainder[..end];
                if !proj.is_empty() { return Some(proj.to_string()); }
            }
        }
    }
    None
}

fn is_valid_project_id(proj: &str) -> bool {
    let p = proj.trim();
    if p.is_empty() || p.len() < 4 || p.len() > 30 {
        return false;
    }
    p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn update_settings_project_id(project_id: &str) -> Result<(), String> {
    let settings_path = format!(
        "{}\\.gemini\\settings.json",
        env::var("USERPROFILE").unwrap_or_default()
    );
    if !std::path::Path::new(&settings_path).exists() {
        let settings_dir = format!(
            "{}\\.gemini",
            env::var("USERPROFILE").unwrap_or_default()
        );
        std::fs::create_dir_all(&settings_dir)
            .map_err(|e| format!("Не удалось создать директорию {}: {}", settings_dir, e))?;
        
        let default_content = format!(
            "{{\n  \"project\": \"{}\"\n}}",
            project_id
        );
        std::fs::write(&settings_path, default_content)
            .map_err(|e| format!("Не удалось записать settings.json: {}", e))?;
        return Ok(());
    }
    
    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Не удалось прочитать settings.json: {}", e))?;
        
    let new_content = if content.contains(r#""project":"#) {
        if let Some(start) = content.find(r#""project":"#) {
            let remainder = &content[start + 10..];
            if let Some(quote_start) = remainder.find('"') {
                let after_quote = &remainder[quote_start + 1..];
                if let Some(quote_end) = after_quote.find('"') {
                    let before = &content[..start + 10 + quote_start + 1];
                    let after = &after_quote[quote_end..];
                    format!("{}{}{}", before, project_id, after)
                } else {
                    content.clone()
                }
            } else {
                content.clone()
            }
        } else {
            content.clone()
        }
    } else {
        if let Some(pos) = content.find('{') {
            let (before, after) = content.split_at(pos + 1);
            format!("{}\n  \"project\": \"{}\",{}", before, project_id, after)
        } else {
            content.clone()
        }
    };
    
    std::fs::write(&settings_path, new_content)
        .map_err(|e| format!("Не удалось обновить settings.json: {}", e))?;
    Ok(())
}

fn get_system_gemini_api_key() -> Option<String> {
    if let Ok(key) = env::var("GEMINI_API_KEY") {
        if is_valid_gemini_api_key(&key) {
            return Some(key.trim().to_string());
        }
    }
    #[cfg(target_os = "windows")]
    {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", "[Environment]::GetEnvironmentVariable('GEMINI_API_KEY', 'User')"])
            .output()
            .ok()?;
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if is_valid_gemini_api_key(&stdout) {
                return Some(stdout);
            }
        }
    }
    None
}

fn handle_patch_antigravity() {
    kill_affected_processes();
    let installs = find_all_installs();

    if installs.is_empty() {
        println!("{}", "Установки Antigravity не найдены.");
        thread::sleep(Duration::from_secs(2));
        return;
    }

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for inst in &installs {
        println!("{}", "--------------------------------------------------");
        println!("{} {}", "Обработка:", mask_path(&inst.display().to_string()));
        match process_install(&inst) {
            Ok(name) => {
                println!("{} {}", "[OK] Успешно пропатчено:", name);
                successes.push(name);
            },
            Err(e) => {
                println!("\x1b[33m[ERR] Ошибка: {}\x1b[0m\x1b[92m", e);
                failures.push(format!("{} - {}", mask_path(&inst.display().to_string()), e));
            }
        }
    }

    // Apply NRPT DNS patch if needed and we are admin
    if (successes.len() > 0 || failures.len() > 0) && is_admin() {
        if !is_nrpt_applied() {
            print!("\nПатч для Google серверов... ");
            io::stdout().flush().ok();
            match setup_dns_nrpt() {
                Ok(_) => println!("OK"),
                Err(_) => println!("пропущено"),
            }
        }
    }

    print_results(&successes, &failures);
}

fn handle_patch_gemini() {
    kill_affected_processes();
    let gemini_cli_exists = is_gemini_cli_installed();

    if !gemini_cli_exists {
        println!("{}", "Gemini CLI не найден.");
        thread::sleep(Duration::from_secs(2));
        return;
    }

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    let mut api_key = String::new();

    // Apply NRPT DNS patch first so generativelanguage endpoint resolves correctly
    if is_admin() && !is_nrpt_applied() {
        print!("\nПатч для Google серверов... ");
        io::stdout().flush().ok();
        match setup_dns_nrpt() {
            Ok(_) => println!("OK"),
            Err(_) => println!("пропущено"),
        }
    }

    let existing_key = get_system_gemini_api_key();

    println!("\n============================================================");
    println!("Gemini CLI (forbidden necromancy)");
    println!("Требуется: AIzaSy-ключ из");
    println!("  {}", link("https://aistudio.google.com/app/u/1/api-keys", "aistudio.google.com/app/u/1/api-keys"));
    println!();

    if let Some(ref ext_key) = existing_key {
        let masked = format!("{}***{}", &ext_key[..6], &ext_key[ext_key.len()-4..]);
        println!("  - Нажмите Enter для использования сохраненного ключа ({})", masked);
        println!("  - Или введите 'skip' для сброса ключа и перехода к браузерному OAuth");
        println!("  - Или вставьте новый AIzaSy-ключ");
    } else {
        println!("  - Вставьте AIzaSy-ключ");
        println!("  - Или нажмите Enter (пустая строка) для пропуска (авторизация через браузер/OAuth)");
    }
    println!("------------------------------------------------------------");

    // Key input loop
    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap_or(0);
        let key_input = input.trim().to_string();

        print!("\x1b[1A\x1b[2K");
        io::stdout().flush().unwrap();

        if key_input.is_empty() {
            if let Some(ref ext_key) = existing_key {
                api_key = ext_key.clone();
                let masked = format!("{}***{}", &api_key[..6], &api_key[api_key.len()-4..]);
                println!("> {}", masked);
                println!("Используется сохраненный API-ключ.");
            } else {
                println!("> (пропущено - будет использоваться авторизация через браузер/OAuth)");
            }
            break;
        }

        if key_input.to_lowercase() == "skip" || key_input.to_lowercase() == "oauth" {
            println!("> (сброшено - будет использоваться авторизация через браузер/OAuth)");
            api_key = String::new();
            break;
        }

        if is_valid_gemini_api_key(&key_input) {
            api_key = key_input;
            let masked = format!("{}***{}", &api_key[..6], &api_key[api_key.len()-4..]);
            println!("> {}", masked);
            println!("API-ключ получен.");
            break;
        } else {
            println!("> (неверный формат)");
            println!("\x1b[33m[ERR] Неверный формат API-ключа. Ожидается AIzaSy (39 символов).\x1b[0m\x1b[92m");
        }
    }

    let mut project_id = String::new();
    let existing_project = get_system_gcloud_project();

    println!("\n============================================================");
    println!("Google Cloud Project ID (Идентификатор проекта)");
    println!("Требуется для работы OAuth (авторизации через браузер).");
    println!("Вы можете получить его из:");
    println!("  {}", link("https://aistudio.google.com/app/apikey", "aistudio.google.com/app/apikey"));
    println!("  (кликните на имя проекта или шестеренку у вашего ключа)");
    println!();

    if let Some(ref ext_proj) = existing_project {
        println!("  - Нажмите Enter для использования сохраненного Project ID ({})", ext_proj);
        println!("  - Или введите 'skip' для сброса и использования дефолтного cloudshell-gca");
        println!("  - Или введите новый Project ID");
    } else {
        println!("  - Введите Project ID");
        println!("  - Или нажмите Enter для пропуска (будет использован дефолтный cloudshell-gca)");
    }
    println!("------------------------------------------------------------");

    // Project ID input loop
    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap_or(0);
        let proj_input = input.trim().to_string();

        print!("\x1b[1A\x1b[2K");
        io::stdout().flush().unwrap();

        if proj_input.is_empty() {
            if let Some(ref ext_proj) = existing_project {
                project_id = ext_proj.clone();
                println!("> {}", project_id);
                println!("Используется сохраненный Project ID.");
            } else {
                println!("> (пропущено - по умолчанию cloudshell-gca)");
            }
            break;
        }

        if proj_input.to_lowercase() == "skip" || proj_input.to_lowercase() == "default" {
            println!("> (сброшено - по умолчанию cloudshell-gca)");
            project_id = String::new();
            break;
        }

        if is_valid_project_id(&proj_input) {
            project_id = proj_input;
            println!("> {}", project_id);
            println!("Project ID получен.");
            break;
        } else {
            println!("> (неверный формат)");
            println!("\x1b[33m[ERR] Неверный формат Project ID. Ожидается: от 4 до 30 символов, строчные латинские буквы, цифры и дефис.\x1b[0m\x1b[92m");
        }
    }

    // Apply Gemini CLI patches
    println!("{}", "--------------------------------------------------");
    println!("Разблокировка Gemini CLI...");

    // Set or clear API key env var
    let set_gemini = if !api_key.is_empty() {
        format!(
            "[Environment]::SetEnvironmentVariable('GEMINI_API_KEY', '{}', 'User')",
            api_key
        )
    } else {
        "[Environment]::SetEnvironmentVariable('GEMINI_API_KEY', $null, 'User')".to_string()
    };
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &set_gemini])
        .output().ok();

    // Set or clear Project ID env var
    let set_project = if !project_id.is_empty() {
        format!(
            "[Environment]::SetEnvironmentVariable('GOOGLE_CLOUD_PROJECT', '{}', 'User')",
            project_id
        )
    } else {
        "[Environment]::SetEnvironmentVariable('GOOGLE_CLOUD_PROJECT', $null, 'User')".to_string()
    };
    Command::new("powershell")
        .args(["-NoProfile", "-Command", &set_project])
        .output().ok();

    // Update settings.json if project_id is provided
    if !project_id.is_empty() {
        if let Err(e) = update_settings_project_id(&project_id) {
            println!("\x1b[33m[ERR] Не удалось обновить settings.json: {}\x1b[0m\x1b[92m", e);
        }
    }

    match run_gemini_patcher() {
        Ok(_) => {
            println!("[OK] Gemini CLI успешно разблокирован!");
            successes.push("Gemini CLI".to_string());
        },
        Err(e) => {
            println!("\x1b[33m[ERR] Ошибка разблокировки Gemini CLI: {}\x1b[0m\x1b[92m", e);
            failures.push(format!("Gemini CLI - {}", e));
        }
    }

    print_results(&successes, &failures);
}

fn print_results(successes: &[String], failures: &[String]) {
    println!("\n{}", "============================================================");
    println!("{}", "ИТОГИ:");
    if !successes.is_empty() {
        println!("{}", "Успешно разблокированы:");
        for s in successes {
            println!("  {} {}", "[+]", s);
        }
    }
    if !failures.is_empty() {
        println!("{}", "Ошибки:");
        for f in failures {
            println!("  \x1b[33m[-] {}\x1b[0m\x1b[92m", f);
        }
    }
    println!("{}", "============================================================");
    println!("{}", "Чтобы вернуться в главное меню, нажмите Enter");
    let mut wait = String::new();
    io::stdin().read_line(&mut wait).unwrap();
}


#[cfg(target_os = "windows")]
mod console_style {
    use std::os::raw::{c_long, c_ushort, c_ulong, c_uint, c_void};
    #[repr(C)] struct COORD { x: c_ushort, y: c_ushort }
    #[repr(C)] struct CONSOLE_FONT_INFOEX { cb_size: c_ulong, n_font: c_ulong, dw_font_size: COORD, font_family: c_uint, font_weight: c_uint, face_name: [u16; 32] }
    extern "system" {
        fn GetStdHandle(nStdHandle: c_ulong) -> *mut c_void;
        fn SetCurrentConsoleFontEx(hConsoleOutput: *mut c_void, bMaximumWindow: c_long, lpConsoleCurrentFontEx: *mut CONSOLE_FONT_INFOEX) -> c_long;
        fn GetConsoleMode(hConsoleHandle: *mut c_void, lpMode: *mut c_ulong) -> c_long;
        fn SetConsoleMode(hConsoleHandle: *mut c_void, dwMode: c_ulong) -> c_long;
        fn SetConsoleTitleW(lpConsoleTitle: *const u16) -> c_long;
    }
    const STD_OUTPUT_HANDLE: c_ulong = 0xFFFFFFF5;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: c_ulong = 0x0004;
    pub fn set(window_title: &str) {
        unsafe {
            std::process::Command::new("cmd").args(["/C", "color 0A"]).status().ok();
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            // Enable VT processing so ANSI escapes (incl. OSC 8 hyperlinks) work in conhost.
            let mut mode: c_ulong = 0;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
            let mut font = CONSOLE_FONT_INFOEX {
                cb_size: std::mem::size_of::<CONSOLE_FONT_INFOEX>() as c_ulong,
                n_font: 0, dw_font_size: COORD { x: 0, y: 20 }, font_family: 54, font_weight: 700, face_name: [0; 32],
            };
            let face = "Consolas";
            for (i, c) in face.encode_utf16().enumerate() { font.face_name[i] = c; }
            SetCurrentConsoleFontEx(handle, 0, &mut font);
            // Set the window title bar so version is visible without taking menu space.
            let mut title_utf16: Vec<u16> = window_title.encode_utf16().collect();
            title_utf16.push(0);
            SetConsoleTitleW(title_utf16.as_ptr());
        }
    }
}

#[cfg(target_os = "windows")]
fn is_admin() -> bool {
    #[link(name = "shell32")]
    extern "system" {
        fn IsUserAnAdmin() -> i32;
    }
    unsafe { IsUserAnAdmin() != 0 }
}

#[cfg(not(target_os = "windows"))]
fn is_admin() -> bool { false }



// Format a URL for display. Standard ANSI formatting is used so that the
// terminal's built-in URI auto-detection recognizes the link.
fn link(url: &str, text: &str) -> String {
    format!("\x1b[94;4m\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\\x1b[0m\x1b[92m", url, text)
}

// Open a URL in the system default browser (Windows: cmd /c start "" <url>).
fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).status().ok();
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open").arg(url).status().ok();
    }
}

// Title shown at the top of the main menu.
const APP_TITLE: &str = "Antigravity Unlocker 2";
// Version is read from Cargo.toml at compile time. Bump version in Cargo.toml
// to bump everywhere (binary file name and any future version display).
const APP_VERSION: &str = "___APP_VERSION___";

fn show_admin_prewarning() {
    clear_screen();
    println!("{}", APP_TITLE);
    println!();
    println!("Внимание: анлокер запущен без прав администратора.");
    println!();
    println!("Без админ-прав будут сняты только клиентские региональные");
    println!("ограничения. Серверный патч требует повышенных привилегий");
    println!("и будет пропущен.");
    println!();
    println!("Если вы находитесь в санкционной территории и упираетесь");
    println!("в 'User location is not supported' — закройте окно и");
    println!("запустите программу от имени Администратора.");
    println!();
    print!("Нажмите Enter чтобы продолжить... ");
    io::stdout().flush().ok();
    let mut tmp = String::new();
    io::stdin().read_line(&mut tmp).ok();
}

fn main() {
    let window_title = format!("Antigravity анлокер v{}", APP_VERSION);
    #[cfg(target_os = "windows")]
    console_style::set(&window_title);

    // Pre-warning shown BEFORE login so user can re-launch as admin if needed.
    if !is_admin() && !is_nrpt_applied() {
        show_admin_prewarning();
    }

    login_screen();

    loop {
        clear_screen();
        println!("{}", APP_TITLE);
        println!();
        println!("1. Разблокировать Antigravity / Antigravity IDE / Antigravity CLI");
        println!("2. Разблокировать Gemini CLI (устарел)");
        println!("3. Отменить NRPT-патч (отключит исправление ошибок \"400\")");
        println!("4. Открыть Telegram-группу ({})", link("https://t.me/nova_txt", "https://t.me/nova_txt"));
        println!("5. Поблагодарить автора ({})", link("https://nova-app.eu/donate", "https://nova-app.eu/donate"));
        println!("0. Выход");
        println!();
        if !is_admin() && !is_nrpt_applied() {
            println!("Запущено без админ-прав: серверный патч будет пропущен.");
            println!();
        }
        print!("> ");
        io::stdout().flush().unwrap();

        let mut choice = String::new();
        io::stdin().read_line(&mut choice).unwrap();

        match choice.trim() {
            "1" => handle_patch_antigravity(),
            "2" => handle_patch_gemini(),
            "3" => handle_restore_dns(),
            "4" => open_url("https://t.me/nova_txt"),
            "5" => open_url("https://nova-app.eu/donate"),
            "0" => break,
            _ => {
                println!("{}", "Неверный выбор.");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
