use std::process::Command;

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

pub fn remove_dns_nrpt() {
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

pub fn is_nrpt_applied() -> bool {
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

pub fn setup_dns_nrpt() -> Result<(), String> {
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
