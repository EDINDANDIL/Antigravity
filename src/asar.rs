use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub fn extract_asar(app_asar: &Path, app_dir: &Path) -> bool {
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
