use crate::adapters::llm::StructuredOutputSchema;
use crate::core::Comment;

use super::VerificationResult;

#[path = "parser/auto_zero.rs"]
mod auto_zero;
#[path = "parser/json.rs"]
mod json;
#[path = "parser/schema.rs"]
mod schema;
#[path = "parser/text.rs"]
mod text;

use auto_zero::apply_auto_zero;
use json::parse_verification_json;
use text::parse_verification_text;

#[allow(unused_imports)]
pub use auto_zero::is_auto_zero;

pub(super) fn verification_response_schema() -> StructuredOutputSchema {
    schema::verification_response_schema()
}

pub(super) fn try_parse_verification_response(
    content: &str,
    comments: &[Comment],
) -> Option<Vec<VerificationResult>> {
    if let Some(results) = parse_verification_json(content, comments) {
        return Some(apply_auto_zero(results, comments));
    }

    let text_results = parse_verification_text(content, comments);
    if text_results.is_empty() {
        None
    } else {
        Some(apply_auto_zero(text_results, comments))
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn parse_verification_response(
    content: &str,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    try_parse_verification_response(content, comments).unwrap_or_default()
}
