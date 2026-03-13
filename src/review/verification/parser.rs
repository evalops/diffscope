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

pub(super) fn parse_verification_response(
    content: &str,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    let results = parse_verification_json(content, comments)
        .unwrap_or_else(|| parse_verification_text(content, comments));
    apply_auto_zero(results, comments)
}
