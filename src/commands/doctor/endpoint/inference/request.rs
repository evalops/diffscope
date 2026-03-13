use serde_json::{json, Value};

pub(super) struct InferenceRequest {
    pub(super) url: String,
    pub(super) body: Value,
}

const SYSTEM_MSG: &str = "You are a code reviewer. Respond with a single JSON object.";
const USER_MSG: &str =
    "Review this code change:\n+fn add(a: i32, b: i32) -> i32 { a + b }\nRespond with: {\"ok\": true}";

pub(super) fn build_inference_request(
    base_url: &str,
    model_name: &str,
    endpoint_type: &str,
) -> InferenceRequest {
    let messages = build_probe_messages();

    if endpoint_type == "ollama" {
        build_ollama_request(base_url, model_name, messages)
    } else {
        build_openai_request(base_url, model_name, messages)
    }
}

fn build_probe_messages() -> Value {
    json!([
        {"role": "system", "content": SYSTEM_MSG},
        {"role": "user", "content": USER_MSG}
    ])
}

fn build_ollama_request(base_url: &str, model_name: &str, messages: Value) -> InferenceRequest {
    InferenceRequest {
        url: format!("{}/api/chat", base_url),
        body: json!({
            "model": model_name,
            "messages": messages,
            "stream": false,
            "options": {"num_predict": 50}
        }),
    }
}

fn build_openai_request(base_url: &str, model_name: &str, messages: Value) -> InferenceRequest {
    InferenceRequest {
        url: format!("{}/v1/chat/completions", base_url),
        body: json!({
            "model": model_name,
            "messages": messages,
            "max_tokens": 50,
            "temperature": 0.1
        }),
    }
}
