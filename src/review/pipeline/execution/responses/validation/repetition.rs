pub(super) fn has_excessive_repetition(text: &str) -> bool {
    if text.len() < 100 {
        return false;
    }
    let window = 20.min(text.len() / 5);
    let search_end = text.len().saturating_sub(window);
    for start in 0..search_end.max(1) {
        if !text.is_char_boundary(start) || !text.is_char_boundary(start + window) {
            continue;
        }
        let pattern = &text[start..start + window];
        if pattern.trim().is_empty() {
            continue;
        }
        if text.matches(pattern).count() > 5 {
            return true;
        }
    }
    false
}
