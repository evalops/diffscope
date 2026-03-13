use std::path::{Path, PathBuf};

pub(super) fn normalize_tool_path(repo_root: &Path, raw: &str) -> PathBuf {
    let raw_components = normalize_path_components(raw);
    if raw_components.is_empty() {
        return PathBuf::from("unknown");
    }

    let repo_components = normalize_path_components(&repo_root.to_string_lossy());
    let repo_components_without_drive = strip_windows_drive_prefix(&repo_components);

    let relative_components = strip_component_prefix(&raw_components, &repo_components)
        .or_else(|| strip_component_prefix(&raw_components, repo_components_without_drive))
        .unwrap_or(raw_components);

    components_to_pathbuf(&relative_components)
}

fn normalize_path_components(raw: &str) -> Vec<String> {
    let normalized = raw.replace('\\', "/");
    let mut components = Vec::new();

    for component in normalized.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            value => components.push(value.to_string()),
        }
    }

    components
}

fn strip_component_prefix(
    path_components: &[String],
    prefix_components: &[String],
) -> Option<Vec<String>> {
    if prefix_components.is_empty() || !path_components.starts_with(prefix_components) {
        return None;
    }

    Some(path_components[prefix_components.len()..].to_vec())
}

fn strip_windows_drive_prefix(components: &[String]) -> &[String] {
    match components {
        [first, rest @ ..] if is_windows_drive(first) => rest,
        _ => components,
    }
}

fn is_windows_drive(component: &str) -> bool {
    let mut chars = component.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(drive), Some(':'), None) if drive.is_ascii_alphabetic()
    )
}

fn components_to_pathbuf(components: &[String]) -> PathBuf {
    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_repo_prefix_from_posix_absolute_paths() {
        assert_eq!(
            normalize_tool_path(Path::new("/repo"), "/repo/src/app.ts"),
            PathBuf::from("src").join("app.ts")
        );
    }

    #[test]
    fn strips_repo_prefix_from_windows_absolute_paths() {
        assert_eq!(
            normalize_tool_path(Path::new(r"C:\repo"), r"C:\repo\src\app.ts"),
            PathBuf::from("src").join("app.ts")
        );
    }

    #[test]
    fn strips_windows_repo_prefix_from_posix_style_tool_paths() {
        assert_eq!(
            normalize_tool_path(Path::new(r"C:\repo"), "/repo/src/app.ts"),
            PathBuf::from("src").join("app.ts")
        );
    }

    #[test]
    fn normalizes_relative_segments_and_backslashes() {
        assert_eq!(
            normalize_tool_path(Path::new("/repo"), r".\src\..\src\ui.tsx"),
            PathBuf::from("src").join("ui.tsx")
        );
    }
}
