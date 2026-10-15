//! Glue shared by both binaries: resolve settings (CLI over config over
//! defaults), fetch, render, and map errors to Waybar output.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;

use crate::api;
use crate::cache::{Cache, DEFAULT_TTL};
use crate::config::{Config, expand_tilde};
use crate::creds;
use crate::error::{AUTH_FAILURE_MESSAGE, AppError, Result};
use crate::fetch::{self, FetchOptions, Outcome};
use crate::pacing::DEFAULT_TOLERANCE;
use crate::render::{self, DEFAULT_FORMAT, DEFAULT_ICON, RenderInput};
use crate::sessions::{self, ProcProbe};
use crate::theme::{ColorOverrides, Theme};
use crate::waybar::WaybarOutput;

/// Values given on the command line; `None` falls through to the config.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub format: Option<String>,
    pub tooltip_format: Option<String>,
    pub icon: Option<String>,
    pub pace_tolerance: Option<u32>,
    pub format_pace_color: bool,
    pub tooltip_pace_pts: bool,
    pub creds_path: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub user_agent: Option<String>,
    pub no_wallbash: bool,
    pub colors: ColorOverrides,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub fetch: FetchOptions,
    pub cache_dir: PathBuf,
    pub claude_dir: PathBuf,
    pub format: String,
    pub tooltip_format: Option<String>,
    /// Empty string = no icon.
    pub icon: String,
    pub pace_tolerance: u32,
    pub format_pace_color: bool,
    pub tooltip_pace_pts: bool,
    pub theme: Theme,
}

impl Settings {
    pub fn resolve(config: &Config, o: Overrides) -> Result<Self> {
        let creds_path = match o.creds_path.or_else(|| config.credentials_path.clone()) {
            Some(p) => expand_tilde(&p),
            None => creds::default_path()?,
        };
        let cache_dir = match o.cache_dir.or_else(|| config.cache_dir.clone()) {
            Some(p) => expand_tilde(&p),
            None => Cache::default_dir()?,
        };
        let colors = ColorOverrides {
            low: o.colors.low.or_else(|| config.colors.low.clone()),
            mid: o.colors.mid.or_else(|| config.colors.mid.clone()),
            high: o.colors.high.or_else(|| config.colors.high.clone()),
            critical: o.colors.critical.or_else(|| config.colors.critical.clone()),
        };
        let use_wallbash = !o.no_wallbash && config.wallbash.unwrap_or(true);
        Ok(Self {
            fetch: FetchOptions {
                creds_path,
                usage_url: api::USAGE_URL.into(),
                user_agent: o
                    .user_agent
                    .or_else(|| config.user_agent.clone())
                    .unwrap_or_else(|| api::DEFAULT_USER_AGENT.into()),
                ttl: DEFAULT_TTL,
            },
            cache_dir,
            claude_dir: creds::claude_dir()?,
            format: o
                .format
                .or_else(|| config.format.clone())
                .unwrap_or_else(|| DEFAULT_FORMAT.into()),
            tooltip_format: o.tooltip_format.or_else(|| config.tooltip_format.clone()),
            icon: o
                .icon
                .or_else(|| config.icon.clone())
                .unwrap_or_else(|| DEFAULT_ICON.into()),
            pace_tolerance: o
                .pace_tolerance
                .or(config.pace_tolerance)
                .unwrap_or(DEFAULT_TOLERANCE),
            format_pace_color: o.format_pace_color || config.format_pace_color.unwrap_or(false),
            tooltip_pace_pts: o.tooltip_pace_pts || config.tooltip_pace_pts.unwrap_or(false),
            theme: Theme::load(use_wallbash, &colors),
        })
    }

    pub fn icon(&self) -> Option<&str> {
        Some(self.icon.as_str()).filter(|i| !i.is_empty())
    }

    pub fn cache(&self) -> Cache {
        Cache::at(self.cache_dir.clone())
    }
}

/// `ttl` = how old a cached payload may be before the API is asked again.
pub async fn fetch(settings: &Settings, ttl: Duration) -> Result<Outcome> {
    let client = api::http_client()?;
    let mut opts = settings.fetch.clone();
    opts.ttl = ttl;
    fetch::fetch_snapshot(&client, &opts, &settings.cache()).await
}

pub fn render(settings: &Settings, outcome: &Outcome) -> WaybarOutput {
    let sessions = sessions::scan(&settings.claude_dir, &ProcProbe);
    render::render(&RenderInput {
        outcome,
        theme: &settings.theme,
        format: &settings.format,
        tooltip_format: settings.tooltip_format.as_deref(),
        icon: settings.icon(),
        pace_tolerance: settings.pace_tolerance,
        format_pace_color: settings.format_pace_color,
        tooltip_pace_pts: settings.tooltip_pace_pts,
        now: Utc::now(),
        sessions: Some(sessions),
    })
}

/// Never fails: every error becomes a `⚠` (or "…" for network hiccups).
pub async fn waybar_output(settings: &Settings, ttl: Duration) -> WaybarOutput {
    match fetch(settings, ttl).await {
        Ok(outcome) => render(settings, &outcome),
        Err(e) if e.is_transient() => WaybarOutput::loading(settings.icon()),
        Err(e) => WaybarOutput::error(&error_message(&e), settings.icon()),
    }
}

/// User-facing text for an error with no cached data to fall back on.
pub fn error_message(e: &AppError) -> String {
    match e {
        AppError::Credentials(m) => format!("Erro de credenciais.\n{m}"),
        AppError::Http { status, .. } if matches!(status, 401 | 403) => {
            format!("HTTP {status}\n{AUTH_FAILURE_MESSAGE}")
        }
        AppError::Http { status, body } => format!("HTTP {status}\n{body}"),
        AppError::Schema(m) => format!("A API mudou de formato.\n{m}"),
        other => other.to_string(),
    }
}
