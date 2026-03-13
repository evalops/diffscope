pub(super) fn validate_structured_review_payload(response: &str) -> Result<bool, String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(response) else {
        return Ok(false);
    };

    if is_structured_review_payload(&value) {
        return Ok(true);
    }

    Err("JSON response did not match the review output contract".to_string())
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
