const COMMENT_PREFIXES: &[&str] = &["//", "# ", "/*", "*/", "* ", "--", "<!--", "\"\"\"", "'''"];

const HASH_NON_COMMENT_PREFIXES: &[&str] = &[
    "#[", "#![", "#!/", "#include", "#define", "#ifdef", "#ifndef", "#endif", "#pragma", "#undef",
    "#elif", "#else", "#if ", "#error", "#warning", "#line",
];

pub(super) fn is_comment_line(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }

    if trimmed.starts_with('#') {
        if HASH_NON_COMMENT_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            return false;
        }
        return true;
    }

    COMMENT_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}
