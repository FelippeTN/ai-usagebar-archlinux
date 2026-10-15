use std::time::{Duration, Instant};

use crate::app::{self, Settings};
use crate::cache::DEFAULT_TTL;
use crate::fetch::Outcome;
use crate::sessions::{self, ProcProbe, SessionActivity};
use crate::waybar;

/// Matches the cache TTL, so an idle dashboard costs at most one request a
/// minute (fewer when the widget already refreshed the cache).
const AUTO_REFRESH: Duration = Duration::from_secs(60);

pub struct App {
    pub settings: Settings,
    /// Last data we managed to get; kept across later failures.
    pub outcome: Option<Outcome>,
    /// Error from the last attempt, when it produced no data at all.
    pub error: Option<String>,
    pub sessions: SessionActivity,
    pub refreshing: bool,
    pub force_refresh: bool,
    pub quit: bool,
    last_attempt: Option<Instant>,
}

impl App {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            outcome: None,
            error: None,
            sessions: SessionActivity::default(),
            refreshing: false,
            force_refresh: false,
            quit: false,
            last_attempt: None,
        }
    }

    /// `Some(force)` when a refresh is due.
    pub fn pending_refresh(&self) -> Option<bool> {
        if self.force_refresh {
            Some(true)
        } else if self
            .last_attempt
            .is_none_or(|t| t.elapsed() >= AUTO_REFRESH)
        {
            Some(false)
        } else {
            None
        }
    }

    pub fn next_refresh_in(&self) -> Duration {
        self.last_attempt
            .map(|t| AUTO_REFRESH.saturating_sub(t.elapsed()))
            .unwrap_or_default()
    }

    /// `force` skips the cache TTL (the 429 backoff still applies) and asks
    /// Waybar to redraw with the new numbers.
    pub fn refresh(&mut self, rt: &tokio::runtime::Runtime, force: bool) {
        let ttl = if force { Duration::ZERO } else { DEFAULT_TTL };
        match rt.block_on(app::fetch(&self.settings, ttl)) {
            Ok(outcome) => {
                let live = outcome.cache_age == Some(Duration::ZERO) && !outcome.stale;
                self.outcome = Some(outcome);
                self.error = None;
                if force && live {
                    waybar::request_refresh();
                }
            }
            Err(e) => self.error = Some(app::error_message(&e)),
        }
        self.sessions = sessions::scan(&self.settings.claude_dir, &ProcProbe);
        self.last_attempt = Some(Instant::now());
        self.refreshing = false;
        self.force_refresh = false;
    }
}
