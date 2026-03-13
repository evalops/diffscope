use serde_json::Value;

use crate::core::offline;

pub(in super::super) fn parse_openai_models(body: &str, models: &mut Vec<offline::LocalModel>) {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(data) = value.get("data").and_then(|d| d.as_array()) {
            for model in data {
                if let Some(id) = model.get("id").and_then(|i| i.as_str()) {
                    models.push(offline::LocalModel {
                        name: id.to_string(),
                        size_mb: 0,
                        quantization: None,
                        modified_at: None,
                        family: None,
                        parameter_size: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openai_models_valid() {
        let body = r#"{"data":[{"id":"gpt-3.5-turbo"},{"id":"codellama-7b"}]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "gpt-3.5-turbo");
        assert_eq!(models[1].name, "codellama-7b");
    }

    #[test]
    fn test_parse_openai_models_empty() {
        let body = r#"{"data":[]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_invalid_json() {
        let body = "not json";
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_missing_data() {
        let body = r#"{"models":[]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_missing_id() {
        let body = r#"{"data":[{"name":"model-1"}]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }
}
