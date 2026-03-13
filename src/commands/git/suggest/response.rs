pub(super) fn extract_title_from_response(content: &str) -> String {
    if let Some(start) = content.find("<title>") {
        let after_tag = start + 7;
        if let Some(end) = content[after_tag..].find("</title>") {
            content[after_tag..after_tag + end].trim().to_string()
        } else {
            content.trim().to_string()
        }
    } else {
        content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title_normal() {
        let content = "<title>Fix login bug</title>";
        assert_eq!(extract_title_from_response(content), "Fix login bug");
    }

    #[test]
    fn test_extract_title_malformed_closing_before_opening() {
        let content = "Some text</title> more <title>Real Title</title>";
        let title = extract_title_from_response(content);
        assert!(!title.is_empty());
    }

    #[test]
    fn test_extract_title_no_tags() {
        let content = "Just a plain title\nSecond line";
        assert_eq!(extract_title_from_response(content), "Just a plain title");
    }
}
