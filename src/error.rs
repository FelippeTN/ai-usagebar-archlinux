use std::path::{Path, PathBuf};

use thiserror::Error;

/// Shown instead of a 401/403 body: those bodies can carry account details.
pub const AUTH_FAILURE_MESSAGE: &str =
    "Token rejeitado pela API. Abra o `claude` para renovar o login.";

/// The widget never refreshes the OAuth token itself (that is Claude Code's
/// job), so an expired token is reported, not repaired.
pub const TOKEN_EXPIRED_MESSAGE: &str =
    "Token expirado. Abra o `claude` para renová-lo; o widget nunca renova o token sozinho.";

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Credentials(String),
    #[error("{}", TOKEN_EXPIRED_MESSAGE)]
    TokenExpired,
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("resposta inesperada da API: {0}")]
    Schema(String),
    #[error("falha de rede: {0}")]
    Transport(String),
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON inválido: {0}")]
    Json(#[from] serde_json::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn io_at(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Network hiccups: callers serve the cache (or "Loading…") silently.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::Transport(e.without_url().to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
