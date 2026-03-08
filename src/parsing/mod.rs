mod llm_response;
mod smart_response;

pub use llm_response::parse_llm_response;
pub use smart_response::parse_smart_review_response;

// Used by sibling modules and their tests
#[allow(unused_imports)]
pub(crate) use llm_response::{extract_rule_id_from_text, extract_rule_id_from_metadata};
#[allow(unused_imports)]
pub(crate) use smart_response::{
    parse_smart_severity, parse_smart_category, parse_smart_confidence,
    parse_smart_effort, parse_smart_tags,
};
