//! Full path: mock endpoint -> fetch -> cache -> Waybar JSON.

use std::time::Duration;

use claude_usagebar::app::{self, Overrides, Settings};
use claude_usagebar::config::Config;

#[tokio::test]
async fn widget_json_from_mock_endpoint() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/api/oauth/usage")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(include_str!("fixtures/usage_full.json"))
        .expect(1)
        .create_async()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let creds = dir.path().join("creds.json");
    let expires = chrono::Utc::now().timestamp_millis() + 3_600_000;
    std::fs::write(
        &creds,
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"t","expiresAt":{expires},"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}}}"#
        ),
    )
    .unwrap();

    let mut settings = Settings::resolve(
        &Config::default(),
        Overrides {
            creds_path: Some(creds),
            cache_dir: Some(dir.path().join("cache")),
            no_wallbash: true,
            ..Default::default()
        },
    )
    .unwrap();
    settings.fetch.usage_url = format!("{}/api/oauth/usage?cedar_ember=1", server.url());

    let out = app::waybar_output(&settings, Duration::from_secs(60)).await;
    let json: serde_json::Value = serde_json::from_str(out.to_json_line().trim()).unwrap();
    assert!(json["text"].as_str().unwrap().contains("62%"));
    assert!(json["tooltip"].as_str().unwrap().contains("Claude Max 20x"));
    assert_eq!(json["class"], serde_json::json!(["mid"]));

    // A second run within the TTL (another monitor) is served from cache.
    let again = app::waybar_output(&settings, Duration::from_secs(60)).await;
    assert_eq!(again.class, out.class);
    mock.assert_async().await;
}

#[tokio::test]
async fn missing_credentials_become_a_warning_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::resolve(
        &Config::default(),
        Overrides {
            creds_path: Some(dir.path().join("nope.json")),
            cache_dir: Some(dir.path().join("cache")),
            no_wallbash: true,
            icon: Some(String::new()),
            ..Default::default()
        },
    )
    .unwrap();
    let out = app::waybar_output(&settings, Duration::from_secs(60)).await;
    assert_eq!(out.text, "⚠");
    assert!(out.tooltip.contains("Erro de credenciais"));
    assert_eq!(out.class, vec!["critical", "error"]);
}
