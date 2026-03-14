use std::path::{Component, Path};

pub fn derive_file_patterns(path: &Path) -> Vec<String> {
    let mut patterns = derive_directory_scope_patterns(path);
    patterns.extend(derive_suffix_patterns(path));
    patterns
}

fn derive_suffix_patterns(path: &Path) -> Vec<String> {
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

fn derive_directory_scope_patterns(path: &Path) -> Vec<String> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };

    let segments = parent
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    if segments.is_empty() {
        return Vec::new();
    }

    let mut patterns = Vec::new();

    for end in (1..=segments.len()).rev() {
        push_unique(&mut patterns, format!("{}/**", segments[..end].join("/")));
    }

    for start in 1..segments.len() {
        push_unique(&mut patterns, format!("{}/**", segments[start..].join("/")));
    }

    patterns
}

fn push_unique(patterns: &mut Vec<String>, pattern: String) {
    if !patterns.contains(&pattern) {
        patterns.push(pattern);
    }
}
