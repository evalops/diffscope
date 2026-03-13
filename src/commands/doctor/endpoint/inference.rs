use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

pub(in super::super) async fn test_model_inference(
    client: &Client,
    base_url: &str,
    model_name: &str,
    endpoint_type: &str,
) -> Result<String> {
    let system_msg = "You are a code reviewer. Respond with a single JSON object.";
    let user_msg =
        "Review this code change:\n+fn add(a: i32, b: i32) -> i32 { a + b }\nRespond with: {\"ok\": true}";

    let messages = serde_json::json!([
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ]);

    if endpoint_type == "ollama" {
        let url = format!("{}/api/chat", base_url);
        let body = serde_json::json!({
            "model": model_name,
            "messages": messages,
            "stream": false,
            "options": {"num_predict": 50}
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} - {}", status, body);
        }

        let text = resp.text().await?;
        parse_ollama_response_content(&text)
    } else {
        let url = format!("{}/v1/chat/completions", base_url);
        let body = serde_json::json!({
            "model": model_name,
            "messages": messages,
            "max_tokens": 50,
            "temperature": 0.1
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} - {}", status, body);
        }

        let text = resp.text().await?;
        parse_openai_response_content(&text)
    }
}

pub(in super::super) fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
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
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("a]"), 1);
    }

    #[test]
    fn test_estimate_tokens_longer_text() {
        let text = "This is a longer response with several words in it for testing.";
        let tokens = estimate_tokens(text);
        assert!(tokens > 10);
        assert!(tokens < 30);
    }

    #[test]
    fn test_test_model_inference_ollama_parse() {
        let json = r#"{"message":{"role":"assistant","content":"{\"ok\": true}"}}"#;
        let content = parse_ollama_response_content(json).unwrap();
        assert_eq!(content, "{\"ok\": true}");
    }

    #[test]
    fn test_test_model_inference_openai_parse() {
        let json = r#"{"choices":[{"message":{"content":"{\"ok\": true}"}}]}"#;
        let content = parse_openai_response_content(json).unwrap();
        assert_eq!(content, "{\"ok\": true}");
    }

    #[test]
    fn test_test_model_inference_empty_choices() {
        let json = r#"{"choices":[]}"#;
        let content = parse_openai_response_content(json).unwrap();
        assert_eq!(content, "");
    }
}
