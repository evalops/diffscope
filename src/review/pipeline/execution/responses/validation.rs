pub(super) fn validate_llm_response(response: &str) -> Result<(), String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return Err("Empty response from model".to_string());
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if is_structured_review_payload(&value) {
            return Ok(());
        }

        return Err("JSON response did not match the review output contract".to_string());
    }

    if response.len() < 10 {
        return Err("Response too short to contain valid review".to_string());
    }

    if has_excessive_repetition(response) {
        return Err("Response contains excessive repetition (model may be stuck)".to_string());
    }

    Ok(())
}

fn is_structured_review_payload(value: &serde_json::Value) -> bool {
    let items = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value
        .get("comments")
        .or_else(|| value.get("findings"))
        .or_else(|| value.get("results"))
        .and_then(|items| items.as_array())
    {
        array
    } else {
        return false;
    };

    items.iter().all(|item| {
        item.is_object()
            && (item.get("line").is_some()
                || item.get("line_number").is_some()
                || item.get("content").is_some()
                || item.get("issue").is_some())
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_response_accepts_valid_response() {
        let response = "Here is my review of the code changes:\n- Line 5: potential null reference";
        assert!(validate_llm_response(response).is_ok());
    }

    #[test]
    fn validate_response_accepts_structured_json() {
        assert!(validate_llm_response("[]").is_ok());
        assert!(validate_llm_response("[{\"line\":10,\"issue\":\"problem\"}]").is_ok());
    }

    #[test]
    fn validate_response_rejects_empty() {
        assert!(validate_llm_response("").is_err());
        assert!(validate_llm_response("   \n\t  ").is_err());
    }

    #[test]
    fn validate_response_rejects_too_short() {
        assert!(validate_llm_response("OK").is_err());
        assert!(validate_llm_response("no issue").is_err());
    }

    #[test]
    fn validate_response_rejects_repetitive() {
        let repeated = "This is a repeating segment.".repeat(20);
        assert!(validate_llm_response(&repeated).is_err());
    }

    #[test]
    fn repetition_short_text_always_false() {
        assert!(!has_excessive_repetition("short"));
        assert!(!has_excessive_repetition(""));
        assert!(!has_excessive_repetition("a".repeat(99).as_str()));
    }

    #[test]
    fn repetition_normal_text_false() {
        let text = "This is a normal code review response. The function looks correct \
                    but there may be an edge case on line 42 where the input could be null. \
                    Consider adding a guard clause to handle this scenario.";
        assert!(!has_excessive_repetition(text));
    }

    #[test]
    fn repetition_stuck_model_detected() {
        let text = "The code looks fine. ".repeat(10);
        assert!(has_excessive_repetition(&text));
    }

    #[test]
    fn repetition_whitespace_only_not_flagged() {
        let text = " ".repeat(200);
        assert!(!has_excessive_repetition(&text));
    }

    #[test]
    fn test_has_excessive_repetition_boundary_120_chars() {
        let pattern = "abcdefghij1234567890";
        let text = pattern.repeat(6);
        assert_eq!(text.len(), 120);
        assert!(has_excessive_repetition(&text));
    }

    #[test]
    fn test_has_excessive_repetition_short_not_detected() {
        let text = "abc".repeat(30);
        assert!(!has_excessive_repetition(&text));
    }
}
