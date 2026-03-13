use serde_json::Value;

use crate::core::Comment;

use super::super::VerificationResult;

pub(super) fn parse_verification_json(
    content: &str,
    comments: &[Comment],
) -> Option<Vec<VerificationResult>> {
    let candidate = extract_json_candidate(content);
    let value = serde_json::from_str::<Value>(&candidate).ok()?;
    let items = value_items(value)?;

    let mut results = Vec::new();
    for item in items {
        let index = item_index(&item)?;
        if index == 0 || index > comments.len() {
            continue;
        }
        results.push(verification_result_from_json_item(
            &item,
            &comments[index - 1],
        ));
    }
    Some(results)
}

fn extract_json_candidate(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("```") {
        trimmed
            .lines()
            .skip_while(|line| line.trim_start().starts_with("```"))
            .take_while(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        trimmed.to_string()
    }
}

fn value_items(value: Value) -> Option<Vec<Value>> {
    if let Some(array) = value.as_array() {
        Some(array.clone())
    } else {
        value
            .get("results")
            .and_then(|results| results.as_array())
            .cloned()
    }
}

fn item_index(item: &Value) -> Option<usize> {
    item.get("index")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
}

fn verification_result_from_json_item(item: &Value, comment: &Comment) -> VerificationResult {
    let accurate = item
        .get("accurate")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let line_correct = item
        .get("line_correct")
        .and_then(|value| value.as_bool())
        .unwrap_or(accurate);
    let suggestion_sound = item
        .get("suggestion_sound")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let score = item
        .get("score")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        .min(10) as u8;
    let reason = item
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or("No reason provided")
        .to_string();

    VerificationResult {
        comment_id: comment.id.clone(),
        accurate,
        line_correct,
        suggestion_sound,
        score,
        reason,
    }
}
