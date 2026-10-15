//! lock → credentials → fresh cache? → token usable? → GET → cache or
//! fallback.
//!
//! Unlike upstream ai-usagebar there is no refresh step: the token is only
//! read. An expired token serves the cached payload marked stale, with a
//! hint to open `claude`, and never touches the network.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;

use crate::api;
use crate::cache::{Cache, acquire_lock};
use crate::creds;
use crate::error::{AppError, Result, TOKEN_EXPIRED_MESSAGE};
use crate::types::UsageResponse;
use crate::usage::ClaudeSnapshot;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(45);
const NO_TOKEN_MESSAGE: &str = "Sem access token. Rode `claude` e faça login.";

#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub creds_path: PathBuf,
    pub usage_url: String,
    pub user_agent: String,
    pub ttl: Duration,
}

/// A snapshot plus how much to trust it.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub snapshot: ClaudeSnapshot,
    /// Not a live answer: an error or backoff is in effect.
    pub stale: bool,
    /// `(http status, message)`; status 0 is a local problem (e.g. token).
    pub last_error: Option<(u16, String)>,
    pub cache_age: Option<Duration>,
}

impl Outcome {
    fn fresh(snapshot: ClaudeSnapshot) -> Self {
        Self {
            snapshot,
            stale: false,
            last_error: None,
            cache_age: Some(Duration::ZERO),
        }
    }

    fn cached(snapshot: ClaudeSnapshot, cache: &Cache, stale: bool) -> Self {
        Self {
            snapshot,
            stale: stale
                || cache.is_stale()
                || cache
                    .backoff_remaining(std::time::SystemTime::now())
                    .is_some(),
            last_error: cache.read_last_error(),
            cache_age: cache.payload_age(),
        }
    }
}

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    opts: &FetchOptions,
    cache: &Cache,
) -> Result<Outcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock(cache.lock_path(), LOCK_TIMEOUT).await?;

    let creds = creds::load(&opts.creds_path)?;
    let plan = creds.plan_label();

    if let Some(bytes) = cache.fresh_payload(opts.ttl)?
        && let Ok(snapshot) = parse(&bytes, &plan)
    {
        return Ok(Outcome::cached(snapshot, cache, false));
    }

    if !creds.has_token() {
        let last = cache.write_last_error(0, NO_TOKEN_MESSAGE);
        return fallback(
            cache,
            Some(last),
            AppError::Credentials(NO_TOKEN_MESSAGE.into()),
            &plan,
        );
    }
    if creds.is_expired(Utc::now().timestamp_millis()) {
        cache.mark_stale();
        let last = cache.write_last_error(0, TOKEN_EXPIRED_MESSAGE);
        return fallback(cache, Some(last), AppError::TokenExpired, &plan);
    }

    let request = api::fetch_usage(
        client,
        &opts.usage_url,
        &creds.access_token,
        &opts.user_agent,
    );
    match tokio::time::timeout(HTTP_TIMEOUT, request).await {
        Ok(Ok(bytes)) => {
            cache.write_payload(&bytes)?;
            Ok(Outcome::fresh(parse(&bytes, &plan)?))
        }
        Ok(Err(AppError::Http { status, body })) => {
            cache.mark_stale();
            let last = cache.write_last_error(status, &body);
            let err = AppError::Http {
                status,
                body: last.1.clone(),
            };
            fallback(cache, Some(last), err, &plan)
        }
        Ok(Err(e)) if e.is_transient() => fallback(cache, None, e, &plan),
        Ok(Err(e)) => {
            cache.mark_stale();
            let last = cache.write_last_error(0, &e.to_string());
            fallback(cache, Some(last), e, &plan)
        }
        Err(_elapsed) => fallback(
            cache,
            None,
            AppError::Transport("timeout na requisição de uso".into()),
            &plan,
        ),
    }
}

fn parse(bytes: &[u8], plan: &str) -> Result<ClaudeSnapshot> {
    let resp: UsageResponse = serde_json::from_slice(bytes)?;
    Ok(resp.into_snapshot(plan.to_string()))
}

/// Serve the last good payload (up to `MAX_STALE`) marked stale, or fail
/// with the original error when there is none.
fn fallback(
    cache: &Cache,
    last_error: Option<(u16, String)>,
    original: AppError,
    plan: &str,
) -> Result<Outcome> {
    let Some(bytes) = cache.fallback_payload()? else {
        return Err(original);
    };
    let Ok(snapshot) = parse(&bytes, plan) else {
        return Err(original);
    };
    let mut outcome = Outcome::cached(snapshot, cache, true);
    if last_error.is_some() {
        outcome.last_error = last_error;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AUTH_FAILURE_MESSAGE;
    use mockito::Matcher;

    const FULL: &str = include_str!("../tests/fixtures/usage_full.json");

    struct Env {
        _dir: tempfile::TempDir,
        cache: Cache,
        opts: FetchOptions,
        creds_path: PathBuf,
    }

    fn env(server: &mockito::Server, expires_at_ms: i64, token: &str) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let creds_path = dir.path().join(".credentials.json");
        let creds = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": token,
                "refreshToken": "rt",
                "expiresAt": expires_at_ms,
                "subscriptionType": "pro",
                "rateLimitTier": "default_claude_ai"
            }
        });
        std::fs::write(&creds_path, creds.to_string()).unwrap();
        Env {
            cache: Cache::at(dir.path().join("cache")),
            opts: FetchOptions {
                creds_path: creds_path.clone(),
                usage_url: format!("{}/api/oauth/usage?cedar_ember=1", server.url()),
                user_agent: api::DEFAULT_USER_AGENT.into(),
                ttl: Duration::from_secs(60),
            },
            creds_path,
            _dir: dir,
        }
    }

    fn valid() -> i64 {
        Utc::now().timestamp_millis() + 3_600_000
    }

    fn expired() -> i64 {
        Utc::now().timestamp_millis() - 1_000
    }

    fn client() -> reqwest::Client {
        api::http_client().unwrap()
    }

    #[tokio::test]
    async fn live_fetch_sends_headers_and_writes_cache() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/api/oauth/usage")
            .match_query(Matcher::UrlEncoded("cedar_ember".into(), "1".into()))
            .match_header("authorization", "Bearer tok")
            .match_header("anthropic-beta", api::BETA_HEADER)
            .match_header("user-agent", api::DEFAULT_USER_AGENT)
            .with_status(200)
            .with_body(FULL)
            .create_async()
            .await;
        let e = env(&server, valid(), "tok");
        let o = fetch_snapshot(&client(), &e.opts, &e.cache).await.unwrap();
        m.assert_async().await;
        assert!(!o.stale);
        assert_eq!(o.snapshot.plan, "Pro");
        assert_eq!(o.snapshot.session.utilization_pct, 62);
        assert!(e.cache.payload_path().exists());
    }

    #[tokio::test]
    async fn fresh_cache_skips_network() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let e = env(&server, valid(), "tok");
        e.cache.write_payload(FULL.as_bytes()).unwrap();
        let o = fetch_snapshot(&client(), &e.opts, &e.cache).await.unwrap();
        m.assert_async().await;
        assert!(!o.stale);
        assert_eq!(o.snapshot.weekly.utilization_pct, 27);
    }

    #[tokio::test]
    async fn expired_token_never_hits_network_nor_touches_creds() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let e = env(&server, expired(), "tok");
        let before = std::fs::read(&e.creds_path).unwrap();

        // No cache: the expiry is the error.
        let err = fetch_snapshot(&client(), &e.opts, &e.cache)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::TokenExpired));

        // With an old cache: served stale, with the hint.
        e.cache.write_payload(FULL.as_bytes()).unwrap();
        let mut opts = e.opts.clone();
        opts.ttl = Duration::ZERO;
        let o = fetch_snapshot(&client(), &opts, &e.cache).await.unwrap();
        assert!(o.stale);
        assert_eq!(o.last_error, Some((0, TOKEN_EXPIRED_MESSAGE.into())));
        assert_eq!(o.snapshot.session.utilization_pct, 62);

        m.assert_async().await;
        assert_eq!(std::fs::read(&e.creds_path).unwrap(), before);
    }

    #[tokio::test]
    async fn blank_token_is_a_credentials_error() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let e = env(&server, valid(), "  ");
        let err = fetch_snapshot(&client(), &e.opts, &e.cache)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
        m.assert_async().await;
    }

    #[tokio::test]
    async fn rate_limit_falls_back_and_backs_off() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", Matcher::Any)
            .with_status(429)
            .with_body(r#"{"error":{"message":"Rate limited"}}"#)
            .expect(1)
            .create_async()
            .await;
        let e = env(&server, valid(), "tok");
        e.cache.write_payload(FULL.as_bytes()).unwrap();
        let mut opts = e.opts.clone();
        opts.ttl = Duration::ZERO;

        let o = fetch_snapshot(&client(), &opts, &e.cache).await.unwrap();
        assert!(o.stale);
        assert_eq!(o.last_error, Some((429, "Rate limited".into())));
        // Second call inside the backoff: no request at all.
        let o = fetch_snapshot(&client(), &opts, &e.cache).await.unwrap();
        assert!(o.stale);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn unauthorized_keeps_plan_and_hides_body() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", Matcher::Any)
            .with_status(401)
            .with_body(r#"{"error":{"message":"invalid token for user@example.com"}}"#)
            .create_async()
            .await;
        let e = env(&server, valid(), "tok");
        let err = fetch_snapshot(&client(), &e.opts, &e.cache)
            .await
            .unwrap_err();
        match err {
            AppError::Http { status, body } => {
                assert_eq!(status, 401);
                assert_eq!(body, AUTH_FAILURE_MESSAGE);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn corrupt_cache_triggers_refetch() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", Matcher::Any)
            .with_status(200)
            .with_body(FULL)
            .expect(1)
            .create_async()
            .await;
        let e = env(&server, valid(), "tok");
        e.cache.write_payload(b"not json").unwrap();
        let o = fetch_snapshot(&client(), &e.opts, &e.cache).await.unwrap();
        m.assert_async().await;
        assert!(!o.stale);
    }

    #[tokio::test]
    async fn schema_drift_is_reported() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", Matcher::Any)
            .with_status(200)
            .with_body(r#"{"five_hour":{"utilization":5000}}"#)
            .create_async()
            .await;
        let e = env(&server, valid(), "tok");
        let err = fetch_snapshot(&client(), &e.opts, &e.cache)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Schema(_)), "{err:?}");
    }
}
