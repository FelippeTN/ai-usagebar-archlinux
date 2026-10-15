//! Optional `~/.config/claude-usagebar/config.toml`. Every key has a default
//! and CLI flags win over the file.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub format: Option<String>,
    pub tooltip_format: Option<String>,
    pub icon: Option<String>,
    pub pace_tolerance: Option<u32>,
    pub format_pace_color: Option<bool>,
    pub tooltip_pace_pts: Option<bool>,
    pub credentials_path: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub user_agent: Option<String>,
    /// Use HyDE's wallbash palette for neutral colors (default true).
    pub wallbash: Option<bool>,
    pub colors: Colors,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    pub low: Option<String>,
    pub mid: Option<String>,
    pub high: Option<String>,
    pub critical: Option<String>,
}

pub fn default_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("claude-usagebar/config.toml"))
}

/// An explicit path must exist; the default one is optional.
pub fn load(explicit: Option<&Path>) -> Result<Config> {
    let (path, required) = match explicit {
        Some(p) => (p.to_path_buf(), true),
        None => match default_path() {
            Some(p) => (p, false),
            None => return Ok(Config::default()),
        },
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => {
            return Ok(Config::default());
        }
        Err(e) => return Err(AppError::io_at(&path, e)),
    };
    parse(&text).map_err(|e| AppError::Config(format!("{}: {e}", path.display())))
}

pub fn parse(text: &str) -> std::result::Result<Config, toml::de::Error> {
    toml::from_str(text)
}

/// `~/x` -> `$HOME/x`.
pub fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => crate::cache::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|_| path.to_path_buf()),
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let c = parse(include_str!("../config.example.toml")).unwrap();
        assert!(c.format.is_some());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(parse("fromat = 'x'").is_err());
        assert!(parse("[colors]\nlow = '#00ff00'").is_ok());
    }

    #[test]
    fn missing_default_is_fine_missing_explicit_is_not() {
        assert!(load(Some(Path::new("/nonexistent/claude-usagebar.toml"))).is_err());
    }
}
