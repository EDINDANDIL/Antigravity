use sha2::{Sha256, Digest};
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use crate::utils::clear_screen;

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

pub fn login_screen() {
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
