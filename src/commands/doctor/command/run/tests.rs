use crate::config::Config;
use serde_json::Value;

#[test]
fn doctor_config_defaults() {
    let config = Config::default();
    assert!(config.adapter.is_none());
    assert!(config.context_window.is_none());
}

#[test]
fn test_detect_context_window_from_parameters() {
    let json = r#"{"parameters":"stop [INST]\nstop [/INST]\nnum_ctx 4096\nrepeat_penalty 1.1"}"#;
    let value: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parse_context_window(&value), Some(4096));
}

#[test]
fn test_detect_context_window_from_model_info() {
    let json = r#"{"model_info":{"llama.context_length":8192}}"#;
    let value: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parse_context_window(&value), Some(8192));
}

#[test]
fn test_detect_context_window_no_data() {
    let json = r#"{"license":"MIT","modelfile":"..."}"#;
    let value: Value = serde_json::from_str(json).unwrap();
    assert_eq!(parse_context_window(&value), None);
}

fn parse_context_window(value: &Value) -> Option<usize> {
    if let Some(params) = value
        .get("parameters")
        .and_then(|parameters| parameters.as_str())
    {
        for line in params.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("num_ctx") {
                if let Some(raw_value) = trimmed.split_whitespace().nth(1) {
                    if let Ok(parsed) = raw_value.parse() {
                        return Some(parsed);
                    }
                }
            }
        }
    }

    let info = value.get("model_info")?;
    for key in &[
        "context_length",
        "llama.context_length",
        "general.context_length",
    ] {
        if let Some(ctx) = info.get(*key).and_then(|value| value.as_u64()) {
            return Some(ctx as usize);
        }
    }

    None
}
