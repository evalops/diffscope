use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use crate::adapters::llm::StructuredOutputSchema;
use crate::core::Comment;

use super::VerificationResult;

const AUTO_ZERO_PATTERNS: &[&str] = &[
    "docstring",
    "doc comment",
    "documentation comment",
    "type hint",
    "type annotation",
    "import order",
    "import sorting",
    "unused import",
    "trailing whitespace",
    "trailing newline",
];

pub fn is_auto_zero(content: &str) -> bool {
    let lower = content.to_lowercase();
    AUTO_ZERO_PATTERNS.iter().any(|p| lower.contains(p))
}

pub(super) fn verification_response_schema() -> StructuredOutputSchema {
    StructuredOutputSchema::json_schema(
        "verification_results",
        serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "index",
                    "accurate",
                    "line_correct",
                    "suggestion_sound",
                    "score",
                    "reason"
                ],
                "properties": {
                    "index": {"type": "integer", "minimum": 1},
                    "accurate": {"type": "boolean"},
                    "line_correct": {"type": "boolean"},
                    "suggestion_sound": {"type": "boolean"},
                    "score": {"type": "integer", "minimum": 0, "maximum": 10},
                    "reason": {"type": "string"}
                }
            }
        }),
    )
}

pub(super) fn parse_verification_response(
    content: &str,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    static FINDING_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)FINDING\s+(\d+)\s*:\s*score\s*=\s*(\d+)\s+accurate\s*=\s*(true|false)(?:\s+line_correct\s*=\s*(true|false))?(?:\s+suggestion_sound\s*=\s*(true|false))?\s+reason\s*=\s*(.+)")
            .unwrap()
    });

    if let Some(results) = parse_verification_json(content, comments) {
        return apply_auto_zero(results, comments);
    }

    let mut results = Vec::new();

    for line in content.lines() {
        if let Some(caps) = FINDING_PATTERN.captures(line) {
            let index: usize = caps
                .get(1)
                .expect("capture group 1 (index) must exist after regex match")
                .as_str()
                .parse()
                .unwrap_or(0);
            let score: u8 = caps
                .get(2)
                .expect("capture group 2 (score) must exist after regex match")
                .as_str()
                .parse()
                .unwrap_or(0);
            let accurate = caps
                .get(3)
                .expect("capture group 3 (accurate) must exist after regex match")
                .as_str()
                .to_lowercase()
                == "true";
            let line_correct = caps
                .get(4)
                .map(|value| value.as_str().eq_ignore_ascii_case("true"))
                .unwrap_or(accurate);
            let suggestion_sound = caps
                .get(5)
                .map(|value| value.as_str().eq_ignore_ascii_case("true"))
                .unwrap_or(true);
            let reason = caps
                .get(6)
                .expect("capture group 6 (reason) must exist after regex match")
                .as_str()
                .trim()
                .to_string();

            if index > 0 && index <= comments.len() {
                results.push(VerificationResult {
                    comment_id: comments[index - 1].id.clone(),
                    accurate,
                    line_correct,
                    suggestion_sound,
                    score: score.min(10),
                    reason,
                });
            }
        }
    }

    apply_auto_zero(results, comments)
}

fn parse_verification_json(content: &str, comments: &[Comment]) -> Option<Vec<VerificationResult>> {
    let trimmed = content.trim();
    let candidate = if trimmed.starts_with("```") {
        trimmed
            .lines()
            .skip_while(|line| line.trim_start().starts_with("```"))
            .take_while(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        trimmed.to_string()
    };

    let value = serde_json::from_str::<Value>(&candidate).ok()?;
    let items = if let Some(array) = value.as_array() {
        array.clone()
    } else {
        value
            .get("results")
            .and_then(|results| results.as_array())
            .cloned()?
    };

    let mut results = Vec::new();
    for item in items {
        let index = item.get("index").and_then(|value| value.as_u64())? as usize;
        if index == 0 || index > comments.len() {
            continue;
        }
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

        results.push(VerificationResult {
            comment_id: comments[index - 1].id.clone(),
            accurate,
            line_correct,
            suggestion_sound,
            score,
            reason,
        });
    }
    Some(results)
}

fn apply_auto_zero(
    mut results: Vec<VerificationResult>,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    for comment in comments {
        if is_auto_zero(&comment.content) {
            if let Some(existing) = results.iter_mut().find(|r| r.comment_id == comment.id) {
                existing.accurate = false;
                existing.line_correct = false;
                existing.score = 0;
                existing.reason = "Auto-zero: noise category".to_string();
            } else {
                results.push(VerificationResult {
                    comment_id: comment.id.clone(),
                    accurate: false,
                    line_correct: false,
                    suggestion_sound: false,
                    score: 0,
                    reason: "Auto-zero: noise category".to_string(),
                });
            }
        }
    }

    results
}
