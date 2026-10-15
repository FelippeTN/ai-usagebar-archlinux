//! Read-only access to Claude Code's OAuth credentials.
//!
//! Claude Code keeps them in `~/.claude/.credentials.json` (or under
//! `$CLAUDE_CONFIG_DIR`). This module only ever reads that file: refreshing
//! the token would rotate the refresh token underneath Claude Code and can
//! log it out, so renewal is left to Claude Code itself.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::error::{AppError, Result};

const MAX_CREDS_BYTES: u64 = 1 << 20;

/// Treat a token as expired this long before `expiresAt`, so a request does
/// not race the deadline.
const EXPIRY_MARGIN_MS: i64 = 30_000;

/// Claude Code's config directory: `$CLAUDE_CONFIG_DIR` or `~/.claude`.
pub fn claude_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(crate::cache::home_dir()?.join(".claude"))
}

pub fn default_path() -> Result<PathBuf> {
    Ok(claude_dir()?.join(".credentials.json"))
}

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth", default)]
    oauth: Option<OauthCreds>,
}

#[derive(Clone, Default, Deserialize)]
pub struct OauthCreds {
    #[serde(rename = "accessToken", default)]
    pub access_token: String,
    /// Unix epoch in milliseconds; int or float on the wire.
    #[serde(rename = "expiresAt", default, deserialize_with = "de_ms_epoch")]
    pub expires_at_ms: Option<i64>,
    /// "pro", "max", …
    #[serde(rename = "subscriptionType", default)]
    pub subscription_type: Option<String>,
    /// "default_claude_ai", "default_claude_max_5x", …
    #[serde(rename = "rateLimitTier", default)]
    pub rate_limit_tier: Option<String>,
}

// Hand-written so a stray `{:?}` can never print the token.
impl fmt::Debug for OauthCreds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OauthCreds")
            .field("access_token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("subscription_type", &self.subscription_type)
            .field("rate_limit_tier", &self.rate_limit_tier)
            .finish()
    }
}

impl OauthCreds {
    /// "Pro", "Max 5x", "Max 20x", or "Unknown". The usage endpoint does not
    /// report the plan, so it comes from the credentials file.
    pub fn plan_label(&self) -> String {
        let mut name =
            crate::format::capitalize(self.subscription_type.as_deref().unwrap_or("").trim());
        if name.is_empty() {
            name = "Unknown".into();
        }
        let tier = self.rate_limit_tier.as_deref().unwrap_or("");
        if tier.contains("20x") {
            name.push_str(" 20x");
        } else if tier.contains("5x") {
            name.push_str(" 5x");
        }
        name
    }

    pub fn has_token(&self) -> bool {
        !self.access_token.trim().is_empty()
    }

    /// A token without `expiresAt` is assumed valid; the API will say if not.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms
            .is_some_and(|expires| expires <= now_ms + EXPIRY_MARGIN_MS)
    }
}

pub fn load(path: &Path) -> Result<OauthCreds> {
    let mut bytes = Vec::new();
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::Credentials(format!(
                "{} não encontrado. Rode `claude` e faça login.",
                path.display()
            )));
        }
        Err(e) => return Err(AppError::io_at(path, e)),
    };
    file.take(MAX_CREDS_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::io_at(path, e))?;
    parse(&bytes).map_err(|msg| AppError::Credentials(format!("{}: {msg}", path.display())))
}

/// Parse errors deliberately omit serde's message: a type error on
/// `accessToken` would quote the value.
fn parse(bytes: &[u8]) -> std::result::Result<OauthCreds, String> {
    let file: CredentialsFile = serde_json::from_slice(bytes).map_err(|e| {
        format!(
            "arquivo ilegível (linha {}, coluna {})",
            e.line(),
            e.column()
        )
    })?;
    file.oauth
        .ok_or_else(|| "sem `claudeAiOauth`. Rode `claude` e faça login.".to_string())
}

fn de_ms_epoch<'de, D>(d: D) -> std::result::Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expiresAt out of range")),
        _ => Err(serde::de::Error::custom("expiresAt must be a number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "claudeAiOauth": {
            "accessToken": "sk-ant-oat01-secret",
            "refreshToken": "sk-ant-ort01-secret",
            "expiresAt": 1792132848843,
            "refreshTokenExpiresAt": 1799999999999,
            "scopes": ["user:inference"],
            "subscriptionType": "pro",
            "rateLimitTier": "default_claude_ai"
        },
        "organizationUuid": "x"
    }"#;

    #[test]
    fn parses_claude_code_file_and_ignores_unknown_fields() {
        let c = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(c.access_token, "sk-ant-oat01-secret");
        assert_eq!(c.expires_at_ms, Some(1_792_132_848_843));
        assert_eq!(c.plan_label(), "Pro");
    }

    #[test]
    fn plan_label_tiers() {
        let mut c = OauthCreds {
            subscription_type: Some("max".into()),
            rate_limit_tier: Some("default_claude_max_5x".into()),
            ..Default::default()
        };
        assert_eq!(c.plan_label(), "Max 5x");
        c.rate_limit_tier = Some("default_claude_max_20x".into());
        assert_eq!(c.plan_label(), "Max 20x");
        c.subscription_type = None;
        c.rate_limit_tier = None;
        assert_eq!(c.plan_label(), "Unknown");
    }

    #[test]
    fn float_and_null_expiry() {
        let c = parse(br#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1.5e12}}"#).unwrap();
        assert_eq!(c.expires_at_ms, Some(1_500_000_000_000));
        let c = parse(br#"{"claudeAiOauth":{"accessToken":"t","expiresAt":null}}"#).unwrap();
        assert_eq!(c.expires_at_ms, None);
        assert!(!c.is_expired(i64::MAX - EXPIRY_MARGIN_MS));
    }

    #[test]
    fn expiry_has_a_margin() {
        let c = OauthCreds {
            expires_at_ms: Some(100_000),
            ..Default::default()
        };
        assert!(!c.is_expired(0));
        assert!(c.is_expired(80_000));
        assert!(c.is_expired(200_000));
    }

    #[test]
    fn errors_never_echo_the_token() {
        let err = parse(br#"{"claudeAiOauth":{"accessToken":12345678}}"#).unwrap_err();
        assert!(!err.contains("12345678"), "{err}");
        let dbg = format!("{:?}", parse(SAMPLE.as_bytes()).unwrap());
        assert!(!dbg.contains("secret"), "{dbg}");
    }

    #[test]
    fn missing_oauth_block_is_a_credentials_error() {
        assert!(parse(b"{}").unwrap_err().contains("claudeAiOauth"));
    }
}
