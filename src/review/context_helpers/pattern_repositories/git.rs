pub(super) fn is_safe_git_url(source: &str) -> bool {
    source.starts_with("https://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || (source.starts_with("http://") && source.ends_with(".git"))
}

pub(super) fn is_git_source(source: &str) -> bool {
    is_safe_git_url(source)
}
