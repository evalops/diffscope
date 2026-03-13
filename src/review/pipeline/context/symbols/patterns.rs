use once_cell::sync::Lazy;
use regex::Regex;

static SYMBOL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([A-Z][a-zA-Z0-9_]*|[a-z][a-zA-Z0-9_]*)\s*\(").unwrap());
static CLASS_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(class|struct|interface|enum)\s+([A-Z][a-zA-Z0-9_]*)").unwrap());

pub(super) fn symbol_regex() -> &'static Regex {
    &SYMBOL_REGEX
}

pub(super) fn class_regex() -> &'static Regex {
    &CLASS_REGEX
}
