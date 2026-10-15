//! The single HTTP call: `GET /api/oauth/usage`.

use std::time::Duration;

use crate::error::{AppError, Result};
use crate::types::UsageResponse;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage?cedar_ember=1";
pub const BETA_HEADER: &str = "oauth-2025-04-20";
/// Load-bearing: without a Claude Code User-Agent the endpoint answers 429,
/// and the `cedar_ember` block checks the surface (`external, cli`) and a
/// minimum CLI version. Bump here (or via config) if that floor moves.
pub const DEFAULT_USER_AGENT: &str = "claude-cli/2.1.281 (external, cli)";
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        // The bearer token must never follow a redirect elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::Other(format!("cliente HTTP: {e}")))
}

/// Returns the raw body, already validated as a `UsageResponse`, so it can be
/// cached as-is.
pub async fn fetch_usage(
    client: &reqwest::Client,
    url: &str,
    access_token: &str,
    user_agent: &str,
) -> Result<Vec<u8>> {
    let mut resp = client
        .get(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("anthropic-beta", BETA_HEADER)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .send()
        .await?;
    let status = resp.status();

    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(AppError::Schema("resposta maior que 2 MiB".into()));
        }
        body.extend_from_slice(&chunk);
    }

    if status.is_success() {
        serde_json::from_slice::<UsageResponse>(&body)
            .map_err(|e| AppError::Schema(format!("payload de uso ilegível: {e}")))?;
        Ok(body)
    } else {
        let text = String::from_utf8_lossy(&body);
        Err(AppError::Http {
            status: status.as_u16(),
            body: parse_error_body(&text).unwrap_or_else(|| text.chars().take(200).collect()),
        })
    }
}

/// `{"error":{"message":…}}`, `{"error":"…"}` or `{"error_description":…}`.
pub fn parse_error_body(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let msg = v
        .get("error_description")
        .and_then(|m| m.as_str())
        .or_else(|| v.pointer("/error/message").and_then(|m| m.as_str()))
        .or_else(|| v.get("error").and_then(|m| m.as_str()))?;
    Some(msg.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_shapes() {
        assert_eq!(
            parse_error_body(r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited"}}"#).as_deref(),
            Some("Rate limited")
        );
        assert_eq!(parse_error_body(r#"{"error":"bad"}"#).as_deref(), Some("bad"));
        assert_eq!(
            parse_error_body(r#"{"error":"x","error_description":"why"}"#).as_deref(),
            Some("why")
        );
        assert_eq!(parse_error_body("<html>"), None);
    }
}
