//! What the local Claude Code sessions are doing right now, from
//! `~/.claude/sessions/<pid>.json`. Feeds the "2 trabalhando · 1 aguardando"
//! tooltip line.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde_json::Value;

const MAX_WALK_ENTRIES: usize = 4_096;
const MAX_SESSION_READS: usize = 256;
const MAX_SESSION_BYTES: u64 = 8 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionActivity {
    pub working: usize,
    pub waiting: usize,
}

impl SessionActivity {
    pub fn summary(&self) -> Option<String> {
        let parts: Vec<String> = [
            (self.working, "trabalhando"),
            (self.waiting, "aguardando você"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, state)| format!("{n} {state}"))
        .collect();
        (!parts.is_empty()).then(|| format!("󰚩  {}", parts.join(" · ")))
    }
}

pub trait ProcessProbe {
    /// `start_time` is field 22 of `/proc/<pid>/stat` as Claude Code
    /// recorded it, which guards against PID reuse.
    fn is_running(&self, pid: u32, start_time: Option<&str>) -> bool;
}

pub struct ProcProbe;

impl ProcessProbe for ProcProbe {
    fn is_running(&self, pid: u32, start_time: Option<&str>) -> bool {
        fs::read_to_string(format!("/proc/{pid}/stat"))
            .is_ok_and(|stat| stat_says_running(&stat, start_time))
    }
}

fn stat_says_running(stat: &str, start_time: Option<&str>) -> bool {
    // The command name (field 2) may contain spaces and parens; split after
    // the last ')'. Then field 3 is the state and field 22 the start time.
    let Some((_, rest)) = stat.rsplit_once(')') else {
        return false;
    };
    let mut fields = rest.split_whitespace();
    let (Some(state), Some(started)) = (fields.next(), fields.nth(18)) else {
        return false;
    };
    !matches!(state, "Z" | "X") && start_time.is_none_or(|expected| expected == started)
}

pub fn scan(claude_dir: &Path, probe: &impl ProcessProbe) -> SessionActivity {
    let mut activity = SessionActivity::default();
    let Ok(entries) = fs::read_dir(claude_dir.join("sessions")) else {
        return activity;
    };
    let mut reads = 0;
    for entry in entries.take(MAX_WALK_ENTRIES).flatten() {
        let Some(pid) = file_pid(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        if reads == MAX_SESSION_READS {
            break;
        }
        reads += 1;
        let Some(session) = read_session(&entry.path()) else {
            continue;
        };
        if session.pid != pid || !session.counts {
            continue;
        }
        let counter = match session.status.as_deref() {
            Some("busy") => &mut activity.working,
            Some("waiting") => &mut activity.waiting,
            _ => continue,
        };
        if probe.is_running(pid, session.start_time.as_deref()) {
            *counter += 1;
        }
    }
    activity
}

struct SessionFile {
    pid: u32,
    /// Interactive, not an SDK run, and on this machine.
    counts: bool,
    status: Option<String>,
    start_time: Option<String>,
}

fn file_pid(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".json")?;
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

fn read_session(path: &Path) -> Option<SessionFile> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SESSION_BYTES {
        return None;
    }
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    let text = |key: &str| v.get(key).and_then(Value::as_str);
    let interactive = text("kind").is_none_or(|k| k == "interactive")
        && text("entrypoint").is_none_or(|e| !e.starts_with("sdk"));
    let local = text("pidDomain").is_none_or(|d| d.split(':').next() == Some("linux"));
    Some(SessionFile {
        pid: u32::try_from(v.get("pid")?.as_u64()?).ok()?,
        counts: interactive && local,
        status: text("status").map(str::to_owned),
        start_time: text("procStart").map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AllAlive;
    impl ProcessProbe for AllAlive {
        fn is_running(&self, pid: u32, _: Option<&str>) -> bool {
            pid != 999
        }
    }

    fn write(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn counts_busy_and_waiting_interactive_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let s = tmp.path().join("sessions");
        fs::create_dir(&s).unwrap();
        write(&s, "1.json", r#"{"pid":1,"kind":"interactive","status":"busy","pidDomain":"linux:abc"}"#);
        write(&s, "2.json", r#"{"pid":2,"kind":"interactive","status":"waiting"}"#);
        write(&s, "3.json", r#"{"pid":3,"kind":"interactive","status":"idle"}"#);
        write(&s, "4.json", r#"{"pid":4,"entrypoint":"sdk-ts","status":"busy"}"#);
        write(&s, "5.json", r#"{"pid":6,"status":"busy"}"#); // pid mismatch
        write(&s, "999.json", r#"{"pid":999,"status":"busy"}"#); // dead
        write(&s, "7.json", r#"{"pid":7,"status":"busy","pidDomain":"darwin:x"}"#);
        write(&s, "8.abc.key", "x");
        let a = scan(tmp.path(), &AllAlive);
        assert_eq!(a, SessionActivity { working: 1, waiting: 1 });
        assert_eq!(a.summary().unwrap(), "󰚩  1 trabalhando · 1 aguardando você");
        assert_eq!(SessionActivity::default().summary(), None);
    }

    #[test]
    fn stat_parsing() {
        // Fields after ')': state(3) ... starttime(22).
        let stat = "123 (claude (x)) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 113999 20";
        assert!(stat_says_running(stat, Some("113999")));
        assert!(stat_says_running(stat, None));
        assert!(!stat_says_running(stat, Some("1")));
        assert!(!stat_says_running(&stat.replace(" S ", " Z "), None));
        assert!(!stat_says_running("garbage", None));
    }

    #[test]
    fn missing_dir_is_idle() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(scan(tmp.path(), &ProcProbe), SessionActivity::default());
    }
}
