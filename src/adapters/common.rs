use anyhow::Result;
use reqwest::StatusCode;
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::sleep;
use url::Url;

/// Send an HTTP request with retry logic for transient failures.
///
/// Retries up to 2 times on retryable status codes (429, 5xx) or connection errors,
/// with linear backoff starting at 250ms.
pub async fn send_with_retry<F>(
    adapter_name: &str,
    mut make_request: F,
) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    const MAX_RETRIES: usize = 2;
    const BASE_DELAY_MS: u64 = 250;

    for attempt in 0..=MAX_RETRIES {
        match make_request().send().await {
            Ok(response) => {
                if response.status().is_success() {
                    return Ok(response);
                }

                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if is_retryable_status(status) && attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(BASE_DELAY_MS * (attempt as u64 + 1))).await;
                    continue;
                }

                let hint = error_hint(status);
                anyhow::bail!(
                    "{} API error ({}): {}\n  Hint: {}",
                    adapter_name,
                    status,
                    body,
                    hint
                );
            }
            Err(err) => {
                if attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(BASE_DELAY_MS * (attempt as u64 + 1))).await;
                    continue;
                }
                return Err(err.into());
            }
        }
    }

    anyhow::bail!("{} request failed after retries", adapter_name);
}

/// Returns a user-facing hint for common HTTP error status codes.
fn error_hint(status: StatusCode) -> &'static str {
    match status.as_u16() {
        401 => "Check that your API key is correct and not expired.",
        403 => "Your API key may lack the required permissions.",
        429 => "Rate limited. Wait a moment and try again, or reduce concurrency.",
        500..=599 => "The API server returned a server error. Try again later.",
        _ => "Check the error message above for details.",
    }
}

/// Returns true if the HTTP status code indicates a transient failure worth retrying.
pub fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Returns true if the given URL points to a local or private endpoint.
///
/// Uses URL parsing to extract the host and checks for localhost, loopback,
/// and private IP ranges. Falls back to string matching if URL parsing fails.
pub fn is_local_endpoint(url_str: &str) -> bool {
    if let Ok(parsed) = Url::parse(url_str) {
        if let Some(host) = parsed.host_str() {
            // Direct hostname checks
            if host == "localhost" {
                return true;
            }

            // Try parsing as an IP address
            // Strip brackets from IPv6 if present (host_str() already does this for Url)
            if let Ok(ip) = host.parse::<IpAddr>() {
                return is_private_or_loopback(ip);
            }

            // Not localhost and not a recognized public API domain -> treat as local/custom
            return !is_known_public_api(host);
        }
    }

    // Fallback: string matching for malformed URLs
    url_str.contains("localhost")
        || url_str.contains("127.0.0.1")
        || url_str.contains("0.0.0.0")
        || url_str.contains("[::1]")
}

/// Check if an IP address is loopback, unspecified, or in a private range.
fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()          // 127.0.0.0/8
                || v4.is_unspecified() // 0.0.0.0
                || v4.is_private()     // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()  // 169.254.0.0/16
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()           // ::1
                || v6.is_unspecified() // ::
        }
    }
}

/// Check if a hostname belongs to a known public LLM API provider.
fn is_known_public_api(host: &str) -> bool {
    host.contains("openai.com")
        || host.contains("anthropic.com")
        || host.contains("googleapis.com")
        || host.contains("azure.com")
        || host.contains("mistral.ai")
        || host.contains("cohere.ai")
        || host.contains("cohere.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn test_is_retryable_status_429() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn test_is_retryable_status_500() {
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn test_is_retryable_status_502() {
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
    }

    #[test]
    fn test_is_retryable_status_503() {
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn test_is_retryable_status_200_not_retryable() {
        assert!(!is_retryable_status(StatusCode::OK));
    }

    #[test]
    fn test_is_retryable_status_400_not_retryable() {
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn test_is_retryable_status_401_not_retryable() {
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn test_is_retryable_status_404_not_retryable() {
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn test_is_local_endpoint_localhost() {
        assert!(is_local_endpoint("http://localhost:11434"));
        assert!(is_local_endpoint("http://localhost:8080/v1"));
        assert!(is_local_endpoint("http://localhost/api"));
    }

    #[test]
    fn test_is_local_endpoint_ipv4_loopback() {
        assert!(is_local_endpoint("http://127.0.0.1:11434"));
        assert!(is_local_endpoint("http://127.0.0.1:8080/v1"));
    }

    #[test]
    fn test_is_local_endpoint_ipv4_unspecified() {
        assert!(is_local_endpoint("http://0.0.0.0:11434"));
    }

    #[test]
    fn test_is_local_endpoint_ipv6_loopback() {
        assert!(is_local_endpoint("http://[::1]:11434"));
    }

    #[test]
    fn test_is_local_endpoint_private_ip() {
        assert!(is_local_endpoint("http://192.168.1.100:8080"));
        assert!(is_local_endpoint("http://10.0.0.5:11434"));
        assert!(is_local_endpoint("http://172.16.0.1:8080"));
    }

    #[test]
    fn test_is_local_endpoint_openai_not_local() {
        assert!(!is_local_endpoint("https://api.openai.com/v1"));
    }

    #[test]
    fn test_is_local_endpoint_anthropic_not_local() {
        assert!(!is_local_endpoint("https://api.anthropic.com/v1"));
    }

    #[test]
    fn test_is_local_endpoint_custom_domain_is_local() {
        // Unknown domains are treated as local/custom endpoints
        assert!(is_local_endpoint("http://my-proxy.internal:8080/v1"));
    }

    #[test]
    fn test_is_local_endpoint_fallback_string_matching() {
        // Malformed URLs fall back to string matching
        assert!(is_local_endpoint("not-a-url-but-has-localhost"));
        assert!(is_local_endpoint("no-scheme-127.0.0.1:8080"));
    }

    #[test]
    fn test_error_hint_401() {
        let hint = error_hint(StatusCode::UNAUTHORIZED);
        assert!(hint.contains("API key"));
    }

    #[test]
    fn test_error_hint_429() {
        let hint = error_hint(StatusCode::TOO_MANY_REQUESTS);
        assert!(hint.contains("Rate limited"));
    }

    #[test]
    fn test_error_hint_500() {
        let hint = error_hint(StatusCode::INTERNAL_SERVER_ERROR);
        assert!(hint.contains("server error"));
    }
}
