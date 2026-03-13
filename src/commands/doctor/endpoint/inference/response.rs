use anyhow::Result;
use serde_json::Value;

pub(super) fn parse_inference_response_content(text: &str, endpoint_type: &str) -> Result<String> {
    if endpoint_type == "ollama" {
        parse_ollama_response_content(text)
    } else {
        parse_openai_response_content(text)
    }
}

fn parse_ollama_response_content(text: &str) -> Result<String> {
    let value: Value = serde_json::from_str(text)?;
    Ok(value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("")
        .to_string())
}

fn parse_openai_response_content(text: &str) -> Result<String> {
    let value: Value = serde_json::from_str(text)?;
    Ok(value
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_inference_response_content_for_ollama() {
        let json = r#"{"message":{"role":"assistant","content":"{\"ok\": true}"}}"#;
        let content = parse_inference_response_content(json, "ollama").unwrap();
        assert_eq!(content, "{\"ok\": true}");
    }

    #[test]
    fn test_parse_inference_response_content_for_openai() {
        let json = r#"{"choices":[{"message":{"content":"{\"ok\": true}"}}]}"#;
        let content = parse_inference_response_content(json, "openai").unwrap();
        assert_eq!(content, "{\"ok\": true}");
    }

    #[test]
    fn test_parse_inference_response_content_for_empty_choices() {
        let json = r#"{"choices":[]}"#;
        let content = parse_inference_response_content(json, "openai").unwrap();
        assert_eq!(content, "");
    }
}
