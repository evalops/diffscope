use super::signals::contains_action_word;
use super::{CodeSuggestion, RawComment};

pub(super) fn generate_code_suggestion(raw: &RawComment) -> Option<CodeSuggestion> {
    if let Some(code_suggestion) = &raw.code_suggestion {
        return Some(code_suggestion.clone());
    }

    if let Some(suggestion) = &raw.suggestion {
        if contains_action_word(suggestion) {
            return Some(CodeSuggestion {
                original_code: "// Original code would be extracted from context".to_string(),
                suggested_code: suggestion.clone(),
                explanation: "Improved implementation following best practices".to_string(),
                diff: format!("- original\n+ {suggestion}"),
            });
        }
    }

    None
}
