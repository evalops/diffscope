use crate::adapters;

pub(super) fn review_comments_response_schema() -> adapters::llm::StructuredOutputSchema {
    adapters::llm::StructuredOutputSchema::json_schema(
        "review_findings",
        serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["line", "content", "severity", "category", "confidence", "fix_effort", "tags"],
                "properties": {
                    "line": {"type": "integer", "minimum": 1},
                    "content": {"type": "string"},
                    "severity": {"type": "string", "enum": ["error", "warning", "info", "suggestion"]},
                    "category": {"type": "string", "enum": ["bug", "security", "performance", "style", "best_practice"]},
                    "confidence": {"type": ["number", "string"]},
                    "fix_effort": {"type": "string", "enum": ["low", "medium", "high"]},
                    "rule_id": {"type": ["string", "null"]},
                    "suggestion": {"type": ["string", "null"]},
                    "code_suggestion": {"type": ["string", "null"]},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                }
            }
        }),
    )
}
