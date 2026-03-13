const LOCK_FILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Gemfile.lock",
    "poetry.lock",
    "composer.lock",
    "go.sum",
    "Pipfile.lock",
];

pub(super) fn is_lock_file(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|file_name| LOCK_FILES.contains(&file_name))
}

pub(super) fn is_generated_file(path: &str) -> bool {
    if path.contains(".generated.")
        || path.contains(".g.")
        || path.starts_with("_generated/")
        || path.contains("/_generated/")
        || path.starts_with("generated/")
        || path.contains("/generated/")
    {
        return true;
    }

    if path.ends_with(".pb.go")
        || path.ends_with(".pb.rs")
        || path.ends_with(".swagger.json")
        || path.ends_with(".min.js")
        || path.ends_with(".min.css")
    {
        return true;
    }

    path.starts_with("vendor/")
}
