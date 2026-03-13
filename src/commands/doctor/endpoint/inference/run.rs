use anyhow::{anyhow, bail, Result};
use reqwest::{Client, Response};

use super::request::{build_inference_request, InferenceRequest};
use super::response::parse_inference_response_content;

pub(in super::super::super) async fn test_model_inference(
    client: &Client,
    base_url: &str,
    model_name: &str,
    endpoint_type: &str,
) -> Result<String> {
    let request = build_inference_request(base_url, model_name, endpoint_type);
    let response_text = execute_inference_request(client, request).await?;
    parse_inference_response_content(&response_text, endpoint_type)
}

async fn execute_inference_request(client: &Client, request: InferenceRequest) -> Result<String> {
    let response = client
        .post(&request.url)
        .json(&request.body)
        .send()
        .await
        .map_err(|error| anyhow!("Request failed: {}", error))?;

    read_inference_response(response).await
}

async fn read_inference_response(response: Response) -> Result<String> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        bail!("HTTP {} - {}", status, text);
    }

    Ok(text)
}

pub(in super::super::super) fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
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
}
