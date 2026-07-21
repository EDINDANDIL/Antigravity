use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub fn patch_binary(_inst: &Path, bin_path: &Path) -> Result<(), String> {
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

#[allow(dead_code)]
pub fn unpatch_binary(bin_path: &Path) -> Result<bool, String> {
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

pub fn kill_affected_processes() {
    println!("\x1b[93m[INFO] Завершаем запущенные процессы перед патчингом...\x1b[0m\x1b[92m");
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

pub fn patch_all_binaries(inst: &Path) {
    let cli = inst.join("agy.exe");
    if cli.exists() {
        match patch_binary(inst, &cli) {
            Ok(_) => println!("  [OK] CLI binary patched"),
            Err(e) => println!("  [ERR] CLI patch failed: {}", e),
        }
    }

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
