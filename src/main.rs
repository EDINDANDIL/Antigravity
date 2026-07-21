use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

mod utils;
mod console_style;
mod auth;
mod dns;
mod asar;
mod patch_binary;
mod patch_ide;
mod patch_gemini;

use utils::{clear_screen, link, open_url, mask_path, is_admin, print_results};
use auth::login_screen;
use dns::{setup_dns_nrpt, remove_dns_nrpt, is_nrpt_applied};
use asar::extract_asar;
use patch_binary::{kill_affected_processes, patch_all_binaries};
use patch_ide::{patch_ide, patch_desktop, patch_extension_js};
use patch_gemini::run_gemini_patcher;

// Title shown at the top of the main menu.
const APP_TITLE: &str = "Antigravity Unlocker 2";
// Version is read from Cargo.toml at compile time. Bump version in Cargo.toml
// to bump everywhere (binary file name and any future version display).
const APP_VERSION: &str = "___APP_VERSION___";

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

fn is_valid_gemini_api_key(key: &str) -> bool {
    key.trim().starts_with("AIzaSy") && key.trim().len() == 39
}

fn get_system_gcloud_project() -> Option<String> {
    if let Ok(proj) = env::var("GOOGLE_CLOUD_PROJECT") {
        if !proj.is_empty() { return Some(proj.trim().to_string()); }
    }
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
    let settings_path = format!(
        "{}\\.gemini\\settings.json",
        env::var("USERPROFILE").unwrap_or_default()
    );
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
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
        match process_install(inst) {
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

    if (!successes.is_empty() || !failures.is_empty()) && is_admin() {
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
            println!("\x1b[33m[ERR] Неверный формат API-ключа. Ожидается: AIzaSy (39 символов).\x1b[0m\x1b[92m");
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

    println!("{}", "--------------------------------------------------");
    println!("Разблокировка Gemini CLI...");

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

    if !is_admin() && !is_nrpt_applied() {
        show_admin_prewarning();
    }

    login_screen();

    loop {
        clear_screen();
        println!("{}", APP_TITLE);
        println!();
        println!("1. Разблокировать Antigravity / Antigravity IDE / Antigravity CLI");
        println!("2. Разблокировать Gemini CLI (deprecated)");
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
