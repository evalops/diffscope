use std::path::Path;

pub fn derive_file_patterns(path: &Path) -> Vec<String> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };

    let parts: Vec<&str> = file_name.split('.').collect();
    if parts.len() < 2 {
        return Vec::new();
    }

    let mut patterns = Vec::new();
    for start in 1..parts.len() {
        let pattern = format!("*.{}", parts[start..].join("."));
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }

    patterns
}
