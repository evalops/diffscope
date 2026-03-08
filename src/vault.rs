use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// Configuration for fetching secrets from HashiCorp Vault.
#[derive(Debug, Clone)]
pub struct VaultConfig {
    /// Vault server address (e.g., https://vault.example.com:8200).
    pub addr: String,
    /// Authentication token.
    pub token: String,
    /// Secret path (e.g., "diffscope" or "ci/diffscope").
    pub path: String,
    /// Key within the secret to extract (e.g., "api_key").
    pub key: String,
    /// KV engine mount point (default: "secret").
    pub mount: String,
    /// Vault Enterprise namespace (optional).
    pub namespace: Option<String>,
}

#[derive(Deserialize)]
struct VaultResponse {
    data: VaultDataWrapper,
}

#[derive(Deserialize)]
struct VaultDataWrapper {
    data: HashMap<String, serde_json::Value>,
}

/// Fetch a single secret value from Vault KV v2.
pub async fn fetch_secret(config: &VaultConfig) -> Result<String> {
    let url = format!("{}/v1/{}/data/{}", config.addr, config.mount, config.path);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build Vault HTTP client")?;

    let mut request = client.get(&url).header("X-Vault-Token", &config.token);

    if let Some(ref ns) = config.namespace {
        request = request.header("X-Vault-Namespace", ns);
    }

    let response = request
        .send()
        .await
        .context("Failed to connect to Vault")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let hint = match status.as_u16() {
            403 => "Check that your Vault token has read access to this path.",
            404 => "Secret path not found. Verify vault_mount and vault_path are correct.",
            _ => "Check your Vault configuration.",
        };
        anyhow::bail!(
            "Vault returned HTTP {}: {}\n  Hint: {}",
            status,
            body.chars().take(200).collect::<String>(),
            hint
        );
    }

    let vault_response: VaultResponse = response
        .json()
        .await
        .context("Failed to parse Vault KV v2 response")?;

    let value = vault_response
        .data
        .data
        .get(&config.key)
        .with_context(|| {
            let available: Vec<&String> = vault_response.data.data.keys().collect();
            format!(
                "Key '{}' not found in Vault secret at {}/{}. Available keys: {:?}",
                config.key, config.mount, config.path, available
            )
        })?;

    match value.as_str() {
        Some(s) => Ok(s.to_string()),
        None => anyhow::bail!(
            "Vault key '{}' is not a string value (got {})",
            config.key,
            value
        ),
    }
}

/// Try to build a VaultConfig from environment variables and config fields.
/// Returns None if Vault is not configured (no addr + path).
pub fn try_build_vault_config(
    vault_addr: Option<&str>,
    vault_token: Option<&str>,
    vault_path: Option<&str>,
    vault_key: Option<&str>,
    vault_mount: Option<&str>,
    vault_namespace: Option<&str>,
) -> Option<VaultConfig> {
    let addr = vault_addr
        .map(|s| s.to_string())
        .or_else(|| std::env::var("VAULT_ADDR").ok())
        .filter(|s| !s.trim().is_empty())?;

    let path = vault_path
        .map(|s| s.to_string())
        .or_else(|| std::env::var("VAULT_PATH").ok())
        .filter(|s| !s.trim().is_empty())?;

    let token = vault_token
        .map(|s| s.to_string())
        .or_else(|| std::env::var("VAULT_TOKEN").ok())
        .filter(|s| !s.trim().is_empty())?;

    let key = vault_key
        .map(|s| s.to_string())
        .or_else(|| std::env::var("VAULT_KEY").ok())
        .unwrap_or_else(|| "api_key".to_string());

    let mount = vault_mount
        .map(|s| s.to_string())
        .unwrap_or_else(|| "secret".to_string());

    let namespace = vault_namespace
        .map(|s| s.to_string())
        .or_else(|| std::env::var("VAULT_NAMESPACE").ok())
        .filter(|s| !s.trim().is_empty());

    Some(VaultConfig {
        addr,
        token,
        path,
        key,
        mount,
        namespace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_build_vault_config_returns_none_without_addr() {
        // Clear env vars for this test
        std::env::remove_var("VAULT_ADDR");
        std::env::remove_var("VAULT_PATH");
        std::env::remove_var("VAULT_TOKEN");
        let result = try_build_vault_config(None, None, None, None, None, None);
        assert!(result.is_none());
    }

    #[test]
    fn try_build_vault_config_returns_none_without_path() {
        std::env::remove_var("VAULT_PATH");
        let result =
            try_build_vault_config(Some("http://vault:8200"), Some("tok"), None, None, None, None);
        assert!(result.is_none());
    }

    #[test]
    fn try_build_vault_config_returns_none_without_token() {
        std::env::remove_var("VAULT_TOKEN");
        let result = try_build_vault_config(
            Some("http://vault:8200"),
            None,
            Some("diffscope"),
            None,
            None,
            None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn try_build_vault_config_builds_with_minimum_fields() {
        let result = try_build_vault_config(
            Some("http://vault:8200"),
            Some("s.mytoken"),
            Some("diffscope"),
            None,
            None,
            None,
        );
        assert!(result.is_some());
        let vc = result.unwrap();
        assert_eq!(vc.addr, "http://vault:8200");
        assert_eq!(vc.token, "s.mytoken");
        assert_eq!(vc.path, "diffscope");
        assert_eq!(vc.key, "api_key"); // default
        assert_eq!(vc.mount, "secret"); // default
        assert!(vc.namespace.is_none());
    }

    #[test]
    fn try_build_vault_config_respects_all_fields() {
        let result = try_build_vault_config(
            Some("https://vault.corp.com:8200"),
            Some("s.tok123"),
            Some("ci/diffscope"),
            Some("openai_key"),
            Some("kv"),
            Some("engineering"),
        );
        assert!(result.is_some());
        let vc = result.unwrap();
        assert_eq!(vc.addr, "https://vault.corp.com:8200");
        assert_eq!(vc.path, "ci/diffscope");
        assert_eq!(vc.key, "openai_key");
        assert_eq!(vc.mount, "kv");
        assert_eq!(vc.namespace.as_deref(), Some("engineering"));
    }

    #[test]
    fn try_build_vault_config_rejects_empty_strings() {
        let result =
            try_build_vault_config(Some("  "), Some("tok"), Some("path"), None, None, None);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn fetch_secret_connection_refused() {
        let config = VaultConfig {
            addr: "http://127.0.0.1:19999".to_string(),
            token: "test".to_string(),
            path: "diffscope".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: None,
        };
        let result = fetch_secret(&config).await;
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("connect") || err.contains("Vault"),
            "Error should mention connection failure: {}",
            err
        );
    }

    #[tokio::test]
    async fn fetch_secret_404_gives_helpful_hint() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/secret/data/diffscope")
            .with_status(404)
            .with_body(r#"{"errors":["no handler for route"]}"#)
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "test-token".to_string(),
            path: "diffscope".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: None,
        };

        let result = fetch_secret(&config).await;
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("404"), "Should contain 404: {}", err);
        assert!(
            err.contains("not found"),
            "Should contain hint: {}",
            err
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_secret_403_gives_permission_hint() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/secret/data/myapp")
            .with_status(403)
            .with_body(r#"{"errors":["permission denied"]}"#)
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "bad-token".to_string(),
            path: "myapp".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: None,
        };

        let result = fetch_secret(&config).await;
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("403"), "Should contain 403: {}", err);
        assert!(
            err.contains("read access"),
            "Should contain token hint: {}",
            err
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_secret_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/secret/data/diffscope")
            .match_header("X-Vault-Token", "s.mytoken")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": {
                        "data": {
                            "api_key": "sk-secret-from-vault",
                            "other": "value"
                        },
                        "metadata": {
                            "version": 1
                        }
                    }
                }"#,
            )
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "s.mytoken".to_string(),
            path: "diffscope".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: None,
        };

        let result = fetch_secret(&config).await;
        assert!(result.is_ok(), "Expected success: {:?}", result);
        assert_eq!(result.unwrap(), "sk-secret-from-vault");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_secret_custom_mount_and_key() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/kv/data/ci/keys")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": {
                        "data": {
                            "openai_key": "sk-from-custom-mount"
                        },
                        "metadata": {"version": 2}
                    }
                }"#,
            )
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "tok".to_string(),
            path: "ci/keys".to_string(),
            key: "openai_key".to_string(),
            mount: "kv".to_string(),
            namespace: None,
        };

        let result = fetch_secret(&config).await;
        assert_eq!(result.unwrap(), "sk-from-custom-mount");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_secret_missing_key_lists_available() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/secret/data/diffscope")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": {
                        "data": {
                            "password": "123",
                            "token": "abc"
                        },
                        "metadata": {"version": 1}
                    }
                }"#,
            )
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "tok".to_string(),
            path: "diffscope".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: None,
        };

        let result = fetch_secret(&config).await;
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("api_key") && err.contains("not found"),
            "Should list the missing key: {}",
            err
        );
        // Should list available keys
        assert!(
            err.contains("password") || err.contains("token"),
            "Should list available keys: {}",
            err
        );
    }

    #[tokio::test]
    async fn fetch_secret_sends_namespace_header() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/secret/data/diffscope")
            .match_header("X-Vault-Namespace", "engineering")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": {
                        "data": {"api_key": "vault-ns-key"},
                        "metadata": {"version": 1}
                    }
                }"#,
            )
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "tok".to_string(),
            path: "diffscope".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: Some("engineering".to_string()),
        };

        let result = fetch_secret(&config).await;
        assert_eq!(result.unwrap(), "vault-ns-key");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_secret_non_string_value_errors() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/secret/data/diffscope")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": {
                        "data": {"api_key": 12345},
                        "metadata": {"version": 1}
                    }
                }"#,
            )
            .create_async()
            .await;

        let config = VaultConfig {
            addr: server.url(),
            token: "tok".to_string(),
            path: "diffscope".to_string(),
            key: "api_key".to_string(),
            mount: "secret".to_string(),
            namespace: None,
        };

        let result = fetch_secret(&config).await;
        assert!(result.is_err());
        assert!(
            format!("{:#}", result.unwrap_err()).contains("not a string"),
            "Should say it's not a string"
        );
    }
}
