use std::env;
use std::io::{self, Write};
use std::process::Command;

/// Runs a PowerShell snippet and hands back the raw output. Shared by the DNS
/// and routing code, which is all cmdlet-driven.
pub fn powershell(script: &str) -> Option<std::process::Output> {
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()
}

pub fn clear_screen() {
    // VT is enabled at startup, so the escape sequence works everywhere and
    // avoids spawning a cmd.exe just to clear the screen.
    print!("\x1b[2J\x1b[3J\x1b[1;1H");
    io::stdout().flush().ok();
}

/// True when the host terminal renders OSC 8 hyperlinks (Windows Terminal and
/// most modern emulators). The legacy conhost window does not, so links are
/// printed as plain text there and opened through the menu instead.
pub fn supports_hyperlinks() -> bool {
    env::var("WT_SESSION").is_ok()
        || env::var("TERM_PROGRAM").is_ok()
        || env::var("ConEmuANSI").map(|v| v == "ON").unwrap_or(false)
}

// Format a URL for display. On terminals that support it the text becomes a
// real hyperlink (Ctrl+Click); elsewhere it stays a readable, selectable URL.
pub fn link(url: &str, text: &str) -> String {
    if supports_hyperlinks() {
        format!(
            "\x1b[94;4m\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\\x1b[0m\x1b[92m",
            url, text
        )
    } else {
        format!("\x1b[94;4m{}\x1b[0m\x1b[92m", text)
    }
}

// Open a URL in the system default browser (Windows: cmd /c start "" <url>).
pub fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .ok();
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open").arg(url).status().ok();
    }
}

/// Prints a prompt and returns the trimmed line the user typed.
pub fn prompt(label: &str) -> String {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap_or(0);
    input.trim().to_string()
}

/// Hint shown next to any printed link, telling the user how to follow it.
pub fn open_hint(keyword: &str) -> String {
    if supports_hyperlinks() {
        format!(
            "(Ctrl+клик по ссылке, либо введите '{}' чтобы открыть в браузере)",
            keyword
        )
    } else {
        format!("(введите '{}' чтобы открыть в браузере)", keyword)
    }
}

pub fn mask_path(path: &str) -> String {
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

#[cfg(target_os = "windows")]
pub fn is_admin() -> bool {
    #[link(name = "shell32")]
    extern "system" {
        fn IsUserAnAdmin() -> i32;
    }
    unsafe { IsUserAnAdmin() != 0 }
}

#[cfg(not(target_os = "windows"))]
pub fn is_admin() -> bool {
    false
}

pub fn print_results(successes: &[String], failures: &[String]) {
    println!(
        "\n{}",
        "============================================================"
    );
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
    println!(
        "{}",
        "============================================================"
    );
    println!("{}", "Чтобы вернуться в главное меню, нажмите Enter");
    let mut wait = String::new();
    io::stdin().read_line(&mut wait).unwrap();
}
