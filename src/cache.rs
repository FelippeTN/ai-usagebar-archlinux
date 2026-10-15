//! On-disk cache shared by every widget instance (one per monitor) and the
//! TUI.
//!
//! ```text
//! ~/.cache/claude-usagebar/usage.json    last good API payload (raw)
//! ~/.cache/claude-usagebar/.stale        marker: payload is not current
//! ~/.cache/claude-usagebar/.last_error   "<code>\n<message>" (code 0 = local)
//! ~/.cache/claude-usagebar/.retry_after  unix seconds; no network before
//! ~/.cache/claude-usagebar/.fetch.lock   flock target
//! ```

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use fs2::FileExt;

use crate::error::{AUTH_FAILURE_MESSAGE, AppError, Result};

/// Several monitors firing at once share one fetch.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);
/// Fallback payloads older than this are not shown.
pub const MAX_STALE: Duration = Duration::from_secs(7 * 24 * 3600);
/// After a 429 the endpoint is left alone for this long.
pub const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn default_dir() -> Result<PathBuf> {
        directories::BaseDirs::new()
            .map(|b| b.cache_dir().join("claude-usagebar"))
            .ok_or_else(|| AppError::Other("não foi possível resolver o diretório de cache".into()))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|e| AppError::io_at(&self.dir, e))
    }

    pub fn payload_path(&self) -> PathBuf {
        self.dir.join("usage.json")
    }
    fn stale_path(&self) -> PathBuf {
        self.dir.join(".stale")
    }
    fn last_error_path(&self) -> PathBuf {
        self.dir.join(".last_error")
    }
    fn retry_after_path(&self) -> PathBuf {
        self.dir.join(".retry_after")
    }
    pub fn lock_path(&self) -> PathBuf {
        self.dir.join(".fetch.lock")
    }

    pub fn payload_age(&self) -> Option<Duration> {
        let mtime = fs::metadata(self.payload_path()).ok()?.modified().ok()?;
        SystemTime::now().duration_since(mtime).ok()
    }

    /// The payload if it is younger than `ttl`. During a 429 backoff any
    /// payload within `MAX_STALE` is returned instead, and with none the
    /// backoff itself is the error: the network must not be touched.
    pub fn fresh_payload(&self, ttl: Duration) -> Result<Option<Vec<u8>>> {
        if let Some(remaining) = self.backoff_remaining(SystemTime::now()) {
            if self.payload_age().is_some_and(|age| age <= MAX_STALE) {
                return self.read_payload().map(Some);
            }
            return Err(AppError::Http {
                status: 429,
                body: format!(
                    "limite de requisições; próxima tentativa em {}",
                    human_backoff(remaining)
                ),
            });
        }
        match self.payload_age() {
            Some(age) if age < ttl => self.read_payload().map(Some),
            _ => Ok(None),
        }
    }

    pub fn fallback_payload(&self) -> Result<Option<Vec<u8>>> {
        match self.payload_age() {
            Some(age) if age <= MAX_STALE => self.read_payload().map(Some),
            _ => Ok(None),
        }
    }

    fn read_payload(&self) -> Result<Vec<u8>> {
        let p = self.payload_path();
        fs::read(&p).map_err(|e| AppError::io_at(&p, e))
    }

    /// A successful write proves the API is fine again: clears every marker.
    pub fn write_payload(&self, bytes: &[u8]) -> Result<()> {
        atomic_write(&self.payload_path(), bytes)?;
        let _ = fs::remove_file(self.stale_path());
        self.clear_last_error();
        Ok(())
    }

    pub fn mark_stale(&self) {
        let _ = self.ensure_dir();
        let _ = File::create(self.stale_path());
    }

    pub fn is_stale(&self) -> bool {
        self.stale_path().exists()
    }

    /// Persist the error and return what was stored. 401/403 bodies are
    /// replaced (they can carry account details); a 429 arms the backoff.
    pub fn write_last_error(&self, code: u16, msg: &str) -> (u16, String) {
        let msg = if matches!(code, 401 | 403) {
            AUTH_FAILURE_MESSAGE.to_string()
        } else {
            crate::pango::sanitize_untrusted(msg)
        };
        let _ = atomic_write(&self.last_error_path(), format!("{code}\n{msg}").as_bytes());
        if code == 429 {
            self.note_rate_limit(SystemTime::now());
        }
        (code, msg)
    }

    pub fn read_last_error(&self) -> Option<(u16, String)> {
        let raw = fs::read_to_string(self.last_error_path()).ok()?;
        let (code, msg) = raw.split_once('\n').unwrap_or((raw.as_str(), ""));
        Some((code.parse().ok()?, msg.to_string()))
    }

    pub fn clear_last_error(&self) {
        let _ = fs::remove_file(self.last_error_path());
        let _ = fs::remove_file(self.retry_after_path());
    }

    pub fn note_rate_limit(&self, now: SystemTime) {
        let until = (now + RATE_LIMIT_BACKOFF)
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = atomic_write(&self.retry_after_path(), until.to_string().as_bytes());
    }

    pub fn backoff_remaining(&self, now: SystemTime) -> Option<Duration> {
        let secs: u64 = fs::read_to_string(self.retry_after_path())
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let until = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))?;
        until.duration_since(now).ok().filter(|d| !d.is_zero())
    }
}

fn human_backoff(remaining: Duration) -> String {
    let secs = remaining.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m", secs.div_ceil(60))
    }
}

/// Exclusive flock, polled so a stuck holder cannot hang Waybar forever.
pub async fn acquire_lock(path: PathBuf, timeout: Duration) -> Result<LockGuard> {
    tokio::task::spawn_blocking(move || acquire_lock_blocking(&path, timeout))
        .await
        .map_err(|e| AppError::Other(format!("lock task failed: {e}")))?
}

fn acquire_lock_blocking(path: &Path, timeout: Duration) -> Result<LockGuard> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io_at(parent, e))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| AppError::io_at(path, e))?;
    let deadline = Instant::now() + timeout;
    loop {
        if file.try_lock_exclusive().is_ok() {
            return Ok(LockGuard { file });
        }
        if Instant::now() >= deadline {
            return Err(AppError::Other(format!(
                "timeout esperando o lock do cache ({timeout:?})"
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// tempfile in the same directory + fsync + rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| AppError::Other(format!("caminho sem diretório: {}", path.display())))?;
    fs::create_dir_all(dir).map_err(|e| AppError::io_at(dir, e))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".tmp.")
        .tempfile_in(dir)
        .map_err(|e| AppError::io_at(dir, e))?;
    tmp.write_all(bytes)
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|e| AppError::io_at(tmp.path(), e))?;
    tmp.persist(path)
        .map_err(|e| AppError::io_at(path, e.error))?;
    Ok(())
}

pub fn home_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .ok_or_else(|| AppError::Other("não foi possível resolver o HOME".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> (tempfile::TempDir, Cache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().join("c"));
        (dir, cache)
    }

    #[test]
    fn fresh_then_expired() {
        let (_d, c) = cache();
        assert!(c.fresh_payload(DEFAULT_TTL).unwrap().is_none());
        c.write_payload(b"{}").unwrap();
        assert_eq!(c.fresh_payload(DEFAULT_TTL).unwrap().unwrap(), b"{}");
        assert!(c.fresh_payload(Duration::ZERO).unwrap().is_none());
        assert!(c.fallback_payload().unwrap().is_some());
    }

    #[test]
    fn write_clears_markers() {
        let (_d, c) = cache();
        c.mark_stale();
        c.write_last_error(500, "boom");
        assert!(c.is_stale());
        assert_eq!(c.read_last_error(), Some((500, "boom".into())));
        c.write_payload(b"{}").unwrap();
        assert!(!c.is_stale());
        assert_eq!(c.read_last_error(), None);
    }

    #[test]
    fn rate_limit_backoff() {
        let (_d, c) = cache();
        c.write_last_error(429, "slow down");
        assert!(c.backoff_remaining(SystemTime::now()).is_some());
        // No payload: the backoff is an error, not a fetch.
        assert!(matches!(
            c.fresh_payload(DEFAULT_TTL),
            Err(AppError::Http { status: 429, .. })
        ));
        // With a payload, even TTL 0 serves it during the backoff.
        c.write_payload(b"{}").unwrap();
        c.write_last_error(429, "slow down");
        assert!(c.fresh_payload(Duration::ZERO).unwrap().is_some());
        let later = SystemTime::now() + RATE_LIMIT_BACKOFF + Duration::from_secs(1);
        assert!(c.backoff_remaining(later).is_none());
    }

    #[test]
    fn auth_errors_are_not_persisted_verbatim() {
        let (_d, c) = cache();
        c.write_last_error(401, "account foo@example.com token abc");
        let (_, msg) = c.read_last_error().unwrap();
        assert_eq!(msg, AUTH_FAILURE_MESSAGE);
    }

    #[test]
    fn multi_line_error_round_trips() {
        let (_d, c) = cache();
        c.write_last_error(500, "line1\nline2");
        assert_eq!(c.read_last_error().unwrap().1, "line1\nline2");
    }
}
