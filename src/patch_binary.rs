use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

// The only edit made to the native binaries: the protobuf field name
// `ineligible` (and `ineligible_tiers`) is renamed to a same-length nonsense
// word. Both the descriptor and the matching Go struct tag are rewritten, so
// they stay consistent; the JSON the client receives no longer carries the
// field it gates on. Same length means offsets, relocations and the PE layout
// are untouched.
//
// The literals live inside obfstr! blocks so they don't show up as plain
// strings in this binary.

fn replace_all(data: &mut [u8], from: &[u8], to: &[u8]) -> usize {
    debug_assert_eq!(from.len(), to.len());
    if data.len() < from.len() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while i + from.len() <= data.len() {
        if &data[i..i + from.len()] == from {
            data[i..i + to.len()].copy_from_slice(to);
            count += 1;
            i += from.len();
        } else {
            i += 1;
        }
    }
    count
}

fn count_occurrences(data: &[u8], needle: &[u8]) -> usize {
    if data.len() < needle.len() {
        return 0;
    }
    (0..=data.len() - needle.len())
        .filter(|&i| &data[i..i + needle.len()] == needle)
        .count()
}

/// Writes the binary back. A running executable is locked on Windows, so the
/// owning process is only killed if the first write actually fails.
fn write_binary(bin_path: &Path, data: &[u8]) -> Result<(), String> {
    if fs::write(bin_path, data).is_ok() {
        return Ok(());
    }
    let file_name = bin_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Command::new("taskkill")
        .args(["/F", "/IM", &file_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok();
    thread::sleep(Duration::from_millis(500));
    fs::write(bin_path, data).map_err(|e| e.to_string())
}

fn rewrite(bin_path: &Path, from: &str, to: &str) -> Result<usize, String> {
    let mut data = fs::read(bin_path).map_err(|e| e.to_string())?;
    let replaced = replace_all(&mut data, from.as_bytes(), to.as_bytes());
    if replaced == 0 {
        // Nothing to do - report whether the target state is already in place.
        return if count_occurrences(&data, to.as_bytes()) > 0 {
            Ok(0)
        } else {
            Err("Сигнатура не найдена".to_string())
        };
    }
    write_binary(bin_path, &data)?;
    Ok(replaced)
}

pub fn patch_binary(_inst: &Path, bin_path: &Path) -> Result<usize, String> {
    obfstr::obfstr! {
        let old_str = "ineligible";
        let new_str = "inexigible";
    }
    rewrite(bin_path, old_str, new_str)
}

pub fn unpatch_binary(bin_path: &Path) -> Result<usize, String> {
    obfstr::obfstr! {
        let old_str = "ineligible";
        let new_str = "inexigible";
    }
    rewrite(bin_path, new_str, old_str)
}

/// Native binaries that carry the eligibility check, for a given install root.
fn binary_targets(inst: &Path) -> Vec<PathBuf> {
    let ext_bin = inst
        .join("resources")
        .join("app")
        .join("extensions")
        .join("antigravity")
        .join("bin");
    [
        inst.join("agy.exe"),
        inst.join("resources")
            .join("bin")
            .join("language_server.exe"),
        ext_bin.join("language_server_windows_x64.exe"),
        ext_bin.join("language_server.exe"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect()
}

pub fn kill_affected_processes() {
    println!("\x1b[93m[INFO] Завершаем запущенные процессы перед патчингом...\x1b[0m\x1b[92m");
    let processes = [
        "Antigravity.exe",
        "Antigravity CLI.exe",
        "Antigravity IDE.exe",
        "agy.exe",
        "language_server.exe",
        "language_server_windows_x64.exe",
    ];
    for p in processes.iter() {
        Command::new("taskkill")
            .args(["/F", "/IM", p])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok();
    }

    // Gemini CLI runs on node. Only the node processes that actually host it
    // are stopped - killing every node.exe would take down unrelated dev
    // servers and editors.
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" -ErrorAction SilentlyContinue | \
             Where-Object { $_.CommandLine -like '*gemini-cli*' } | \
             ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok();

    thread::sleep(Duration::from_millis(1000));
}

/// Outcome of patching the native binaries of one install.
pub struct BinarySummary {
    /// Binaries that are now in the patched state (freshly patched or already so).
    pub ok: usize,
    /// Binaries where the signature could not be found or written.
    pub failed: usize,
}

impl BinarySummary {
    pub fn total(&self) -> usize {
        self.ok + self.failed
    }
}

pub fn patch_all_binaries(inst: &Path) -> BinarySummary {
    let mut summary = BinarySummary { ok: 0, failed: 0 };
    for bin in binary_targets(inst) {
        let label = bin
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        match patch_binary(inst, &bin) {
            Ok(0) => {
                println!("  [OK] {} — уже пропатчен", label);
                summary.ok += 1;
            }
            Ok(n) => {
                println!("  [OK] {} — заменено вхождений: {}", label, n);
                summary.ok += 1;
            }
            Err(e) => {
                println!("  [ERR] {}: {}", label, e);
                summary.failed += 1;
            }
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_every_occurrence() {
        let mut data = b"xxineligibleyyineligible".to_vec();
        assert_eq!(replace_all(&mut data, b"ineligible", b"inexigible"), 2);
        assert_eq!(&data, b"xxinexigibleyyinexigible");
    }

    #[test]
    fn matches_at_the_very_end_of_the_buffer() {
        // The previous implementation used `0..len - needle.len()`, which
        // silently skipped a match sitting at the last possible offset.
        let mut data = b"padineligible".to_vec();
        assert_eq!(replace_all(&mut data, b"ineligible", b"inexigible"), 1);
        assert_eq!(&data, b"padinexigible");
    }

    #[test]
    fn shorter_than_needle_is_not_a_panic() {
        let mut data = b"abc".to_vec();
        assert_eq!(replace_all(&mut data, b"ineligible", b"inexigible"), 0);
        assert_eq!(count_occurrences(&data, b"ineligible"), 0);
    }

    /// End-to-end check against the installed Language Server: patch a copy,
    /// confirm the signature is found, the file size is untouched and the
    /// revert is byte-exact. Heavy (copies ~140 MB), so run it explicitly:
    ///   cargo test --bin ag_unlocker -- --ignored
    #[test]
    #[ignore]
    fn patches_and_reverts_the_installed_language_server() {
        let Ok(local) = std::env::var("LOCALAPPDATA") else {
            return;
        };
        let src = Path::new(&local)
            .join("Programs")
            .join("Antigravity")
            .join("resources")
            .join("bin")
            .join("language_server.exe");
        if !src.exists() {
            return;
        }

        let tmp = std::env::temp_dir().join("ag_unlocker_ls_patch_test.exe");
        fs::copy(&src, &tmp).expect("copy language server");
        let original = fs::read(&src).expect("read original");

        let patched = patch_binary(Path::new(""), &tmp).expect("signature found");
        assert!(patched > 0, "no occurrences replaced");
        let after = fs::read(&tmp).expect("read patched");
        assert_eq!(after.len(), original.len(), "patch changed the file size");
        assert_eq!(count_occurrences(&after, b"ineligible"), 0);
        assert_eq!(count_occurrences(&after, b"inexigible"), patched);

        // Re-running must be a no-op, not an error.
        assert_eq!(patch_binary(Path::new(""), &tmp).expect("idempotent"), 0);

        assert_eq!(unpatch_binary(&tmp).expect("revert"), patched);
        assert_eq!(fs::read(&tmp).expect("read reverted"), original);

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn round_trip_restores_the_original_bytes() {
        let original = b"a ineligible b ineligible_tiers c".to_vec();
        let mut data = original.clone();
        replace_all(&mut data, b"ineligible", b"inexigible");
        assert_ne!(data, original);
        replace_all(&mut data, b"inexigible", b"ineligible");
        assert_eq!(data, original);
    }
}

/// Reverses the binary patch so an install can be returned to stock without
/// reinstalling.
pub fn unpatch_all_binaries(inst: &Path) -> usize {
    let mut reverted = 0;
    for bin in binary_targets(inst) {
        let label = bin
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        match unpatch_binary(&bin) {
            Ok(0) => println!("  [--] {} — патч не найден", label),
            Ok(n) => {
                println!("  [OK] {} — возвращено вхождений: {}", label, n);
                reverted += 1;
            }
            Err(e) => println!("  [ERR] {}: {}", label, e),
        }
    }
    reverted
}
