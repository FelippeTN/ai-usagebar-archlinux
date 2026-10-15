use chrono::{DateTime, Utc};

/// `reset - now` as a short string: `—` (unknown), `agora` (passed),
/// `2d 3h`, or `1h 05m`.
pub fn format(reset: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(reset) = reset else {
        return "—".to_string();
    };
    let secs = reset.signed_duration_since(now).num_seconds();
    if secs <= 0 {
        return "agora".to_string();
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{hours}h {mins:02}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn buckets() {
        let now = Utc.with_ymd_and_hms(2026, 5, 23, 12, 0, 0).unwrap();
        let at = |s: i64| Some(now + chrono::Duration::seconds(s));
        assert_eq!(format(None, now), "—");
        assert_eq!(format(at(0), now), "agora");
        assert_eq!(format(at(-60), now), "agora");
        assert_eq!(format(at(90 * 60), now), "1h 30m");
        assert_eq!(format(at(5 * 60), now), "0h 05m");
        assert_eq!(format(at(2 * 86_400 + 3 * 3_600 + 59), now), "2d 3h");
    }
}
