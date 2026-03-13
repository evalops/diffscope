pub(in super::super) fn summarize_for_eval(content: &str) -> String {
    let mut summary = content.trim().replace('\n', " ");
    if summary.len() > 120 {
        let mut end = 117;
        while end > 0 && !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
        summary.push_str("...");
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_for_eval_short() {
        let result = summarize_for_eval("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_summarize_for_eval_utf8_safety() {
        let content = format!("a{}{}", "€".repeat(39), "abc");
        let result = summarize_for_eval(&content);
        assert!(result.len() <= 120);
    }
}
