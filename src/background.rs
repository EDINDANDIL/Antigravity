use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::dns_forwarder;
use crate::utils::powershell;

// Keeping the DNS relay alive across reboots.
//
// A scheduled task rather than a real service: a Windows service has to
// implement StartServiceCtrlDispatcher and a control handler or the SCM kills
// the process after ~30 seconds, which is a protocol to maintain for exactly
// the same result. A logon-triggered task is a plain background process, and
// enable/disable is one cmdlet each.
//
// Registered through the ScheduledTasks cmdlets, not schtasks.exe: the defaults
// of the latter are wrong here in two ways that only surface on a laptop weeks
// later - a task is stopped when the machine goes on battery, and it is killed
// after a 72-hour execution limit. Both are switched off explicitly below.

const TASK_NAME: &str = "AG Unlocker DNS";
const EXE_NAME: &str = "ag_dns.exe";
/// Hidden flag that turns the unlocker into the relay. Handled before any UI.
pub const FORWARDER_FLAG: &str = "--dns-forwarder";

/// ProgramData, **not** LOCALAPPDATA. Measured on a real machine: a scheduled
/// task launching anything out of `%LOCALAPPDATA%` fails with 0x80070002, and a
/// stock `ping.exe` copied there fails identically - it is an anti-persistence
/// heuristic (a task autostarting an exe from AppData is the classic malware
/// shape), not something about our binary. The same probe from ProgramData and
/// Program Files starts cleanly.
pub fn install_dir() -> PathBuf {
    PathBuf::from(env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string()))
        .join("AGUnlocker")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join(EXE_NAME)
}

/// True when the logon task exists. Says nothing about whether the relay is
/// running right now - `is_running` answers that.
pub fn is_enabled() -> bool {
    let cmd = format!(
        "if (Get-ScheduledTask -TaskName '{}' -ErrorAction SilentlyContinue) {{ 'yes' }} else {{ 'no' }}",
        TASK_NAME
    );
    powershell(&cmd).map_or(false, |o| {
        String::from_utf8_lossy(&o.stdout).trim() == "yes"
    })
}

pub fn is_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {}", EXE_NAME), "/NH"])
        .output()
        .map_or(false, |o| {
            String::from_utf8_lossy(&o.stdout).contains(EXE_NAME)
        })
}

fn stop_process() {
    Command::new("taskkill")
        .args(["/F", "/IM", EXE_NAME])
        .output()
        .ok();
}

/// Copies this exe next to its log and registers the logon task. The copy is
/// what makes autostart survive the user moving or deleting the download; it is
/// removed again by `disable`.
pub fn enable() -> Result<(), String> {
    let src = env::current_exe().map_err(|e| format!("не найден путь к exe: {}", e))?;
    let dir = install_dir();
    let dst = installed_exe();

    fs::create_dir_all(&dir).map_err(|e| format!("не создать {}: {}", dir.display(), e))?;
    // The file cannot be replaced while the previous relay holds it open.
    stop_process();
    if src != dst {
        fs::copy(&src, &dst).map_err(|e| format!("не скопировать exe: {}", e))?;
    }

    let cmd = format!(
        "$ErrorActionPreference='Stop'; \
         $a=New-ScheduledTaskAction -Execute '{exe}' -Argument '{flag}'; \
         $t=New-ScheduledTaskTrigger -AtLogOn; \
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries \
              -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew \
              -ExecutionTimeLimit ([TimeSpan]::Zero); \
         Register-ScheduledTask -TaskName '{task}' -Action $a -Trigger $t -Settings $s \
              -Description 'Antigravity Unlocker: локальный DNS-релей' -Force | Out-Null; \
         Start-ScheduledTask -TaskName '{task}'",
        exe = dst.display(),
        flag = FORWARDER_FLAG,
        task = TASK_NAME
    );

    let out = powershell(&cmd).ok_or_else(|| "не удалось запустить PowerShell".to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "не удалось зарегистрировать задачу".to_string()
        } else {
            stderr
        });
    }
    Ok(())
}

/// Enables the relay, or restarts it if the task exists but nothing is running.
/// Cheap when it is already up, so the patch flow can call it every time.
pub fn ensure_running() -> Result<(), String> {
    if is_enabled() && is_running() {
        return Ok(());
    }
    enable()
}

pub fn disable() -> Result<(), String> {
    let cmd = format!(
        "Stop-ScheduledTask -TaskName '{task}' -ErrorAction SilentlyContinue; \
         Unregister-ScheduledTask -TaskName '{task}' -Confirm:$false -ErrorAction SilentlyContinue",
        task = TASK_NAME
    );
    powershell(&cmd);
    stop_process();
    fs::remove_file(installed_exe()).ok();
    fs::remove_file(dns_forwarder::log_path()).ok();
    // Both only succeed while the directory is empty, which is what we want.
    fs::remove_dir(install_dir()).ok();
    fs::remove_dir(dns_forwarder::log_dir()).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exe must not sit under the user profile: a scheduled task cannot
    /// launch anything from there on a machine with anti-persistence heuristics.
    #[test]
    fn the_relay_is_installed_outside_the_user_profile() {
        let exe = installed_exe();
        assert_eq!(exe.parent(), Some(install_dir().as_path()));

        let program_data = env::var("ProgramData").unwrap_or_default();
        assert!(!program_data.is_empty());
        assert!(exe.starts_with(&program_data), "got {}", exe.display());

        if let Ok(local) = env::var("LOCALAPPDATA") {
            assert!(
                !exe.starts_with(&local),
                "the task would refuse to start it"
            );
        }
    }

    /// The log goes the other way round - the relay runs unelevated and cannot
    /// write next to an exe an administrator installed.
    #[test]
    fn the_log_stays_in_the_user_profile() {
        let log = dns_forwarder::log_path();
        let local = env::var("LOCALAPPDATA").unwrap_or_default();
        assert!(!local.is_empty());
        assert!(log.starts_with(&local), "got {}", log.display());
        assert_eq!(log.parent(), Some(dns_forwarder::log_dir().as_path()));
        assert_ne!(log.parent(), Some(install_dir().as_path()));
    }
}
