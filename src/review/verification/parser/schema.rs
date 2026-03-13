use crate::adapters::llm::StructuredOutputSchema;

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
