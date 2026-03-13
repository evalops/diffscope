use std::collections::HashMap;
use std::path::PathBuf;

#[path = "pattern_repositories/checkout.rs"]
mod checkout;
#[path = "pattern_repositories/git.rs"]
mod git;
#[path = "pattern_repositories/local.rs"]
mod local;
#[path = "pattern_repositories/run.rs"]
mod run;

pub type PatternRepositoryMap = HashMap<String, PathBuf>;

pub use run::resolve_pattern_repositories;

#[cfg(test)]
mod tests {
    use super::git::{is_git_source, is_safe_git_url};

    #[test]
    fn test_is_git_source_https() {
        assert!(is_git_source("https://github.com/org/repo.git"));
        assert!(is_git_source("https://github.com/org/repo"));
    }

    #[test]
    fn test_is_git_source_ssh() {
        assert!(is_git_source("git@github.com:org/repo.git"));
    }

    #[test]
    fn test_is_git_source_http_with_git_suffix() {
        assert!(is_git_source("http://example.com/repo.git"));
    }

    #[test]
    fn test_is_git_source_rejects_local_paths() {
        assert!(!is_git_source("/tmp/evil"));
        assert!(!is_git_source("../relative/path"));
        assert!(!is_git_source("file:///etc/passwd"));
    }

    #[test]
    fn test_is_git_source_rejects_other_schemes() {
        assert!(!is_git_source("ftp://example.com/repo.git"));
    }

    #[test]
    fn test_is_git_source_accepts_ssh() {
        assert!(is_git_source("ssh://example.com/repo"));
    }

    #[test]
    fn test_is_safe_git_url_allows_https() {
        assert!(is_safe_git_url("https://github.com/org/repo"));
        assert!(is_safe_git_url("https://gitlab.com/org/repo.git"));
    }

    #[test]
    fn test_is_safe_git_url_allows_ssh() {
        assert!(is_safe_git_url("git@github.com:org/repo.git"));
        assert!(is_safe_git_url("ssh://example.com/repo"));
        assert!(is_safe_git_url("ssh://git@gitlab.internal/org/rules.git"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_file_urls() {
        assert!(!is_safe_git_url("file:///etc/passwd"));
        assert!(!is_safe_git_url("/tmp/evil"));
        assert!(!is_safe_git_url("../traversal"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_arbitrary_schemes() {
        assert!(!is_safe_git_url("ftp://example.com/repo"));
        assert!(!is_safe_git_url("gopher://example.com/repo"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_http_without_git_suffix() {
        assert!(!is_safe_git_url("http://example.com/repo"));
    }
}
