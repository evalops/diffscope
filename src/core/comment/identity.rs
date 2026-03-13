use std::path::Path;

use super::Category;

pub fn compute_comment_id(file_path: &Path, content: &str, category: &Category) -> String {
    let normalized = normalize_content(content);
    let key = format!("{}|{}|{}", file_path.display(), category, normalized);
    let hash = fnv1a64(key.as_bytes());
    format!("cmt_{:016x}", hash)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn normalize_content(content: &str) -> String {
    let mut normalized = String::new();
    let mut last_space = false;

    for ch in content.chars() {
        let ch = if ch.is_ascii_digit() {
            '#'
        } else {
            ch.to_ascii_lowercase()
        };

        if ch.is_whitespace() {
            if !last_space {
                normalized.push(' ');
                last_space = true;
            }
        } else {
            normalized.push(ch);
            last_space = false;
        }
    }

    normalized.trim().to_string()
}
