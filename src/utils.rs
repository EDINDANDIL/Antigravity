use std::env;
use std::io;
use std::process::Command;

pub fn clear_screen() {
    if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "cls"]).status().unwrap();
    } else {
        print!("{}[2J{}[1;1H", 27 as char, 27 as char);
    }
}

// Format a URL for display. Standard ANSI formatting is used so that the
// terminal's built-in URI auto-detection recognizes the link.
pub fn link(url: &str, text: &str) -> String {
    format!("\x1b[94;4m\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\\x1b[0m\x1b[92m", url, text)
}

// Open a URL in the system default browser (Windows: cmd /c start "" <url>).
pub fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).status().ok();
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open").arg(url).status().ok();
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
pub fn is_admin() -> bool { false }

pub fn print_results(successes: &[String], failures: &[String]) {
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
