//! Pacing: is usage running ahead of the clock?
//!
//! Two views of the same question:
//! - **ratio** `usage / elapsed`, with a tolerance band, for `{*_pace}` and
//!   `{*_pace_pct}`;
//! - **point delta** `usage - elapsed`, no tolerance, for
//!   `{*_pace_indicator}`, `{*_pace_pts}` and `{*_pace_delta}`.

use chrono::{DateTime, Utc};

use crate::usage::Severity;

pub const DEFAULT_TOLERANCE: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Ahead,
    OnTrack,
    Under,
}

impl Pace {
    pub fn glyph(self) -> &'static str {
        match self {
            Pace::Ahead => "↑",
            Pace::OnTrack => "→",
            Pace::Under => "↓",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacing {
    /// Percent of the window already elapsed (0..=100).
    pub elapsed_pct: i32,
    pub ratio_pace: Pace,
    pub point_pace: Pace,
    /// `usage_pct - elapsed_pct`.
    pub delta: i32,
    /// "12% acima" / "5% abaixo" / "no ritmo".
    pub ratio_label: String,
    /// "12pts acima" / "5pts abaixo" / "no ritmo".
    pub point_label: String,
}

impl Pacing {
    /// For windows without a reset time.
    pub fn neutral() -> Self {
        Self {
            elapsed_pct: 0,
            ratio_pace: Pace::OnTrack,
            point_pace: Pace::OnTrack,
            delta: 0,
            ratio_label: "no ritmo".into(),
            point_label: "no ritmo".into(),
        }
    }
}

pub fn calc(
    usage_pct: i32,
    reset: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    window: chrono::Duration,
    tolerance: u32,
) -> Pacing {
    let Some(reset) = reset else {
        return Pacing::neutral();
    };
    let total = window.num_seconds();
    if total <= 0 {
        return Pacing::neutral();
    }
    let remaining = reset.signed_duration_since(now).num_seconds();
    let elapsed_pct = (((total - remaining) * 100) / total).clamp(0, 100) as i32;

    let delta = usage_pct - elapsed_pct;
    let (point_pace, point_label) = match delta {
        d if d > 0 => (Pace::Ahead, format!("{d}pts acima")),
        d if d < 0 => (Pace::Under, format!("{}pts abaixo", -d)),
        _ => (Pace::OnTrack, "no ritmo".to_string()),
    };

    let (ratio_pace, ratio_label) = if elapsed_pct > 0 {
        let ratio_x100 = (usage_pct * 100) / elapsed_pct;
        let tol = tolerance as i32;
        if ratio_x100 > 100 + tol {
            let dev = (ratio_x100 - 100).min(999);
            (Pace::Ahead, format!("{dev}% acima"))
        } else if ratio_x100 < 100 - tol {
            let dev = (100 - ratio_x100).min(999);
            (Pace::Under, format!("{dev}% abaixo"))
        } else {
            (Pace::OnTrack, "no ritmo".to_string())
        }
    } else {
        (Pace::OnTrack, "no ritmo".to_string())
    };

    Pacing {
        elapsed_pct,
        ratio_pace,
        point_pace,
        delta,
        ratio_label,
        point_label,
    }
}

/// `>= 10` critical, `1..=9` high, `-10..=0` mid, `< -10` low.
pub fn pace_severity(delta: i32) -> Severity {
    if delta >= 10 {
        Severity::Critical
    } else if delta > 0 {
        Severity::High
    } else if delta >= -10 {
        Severity::Mid
    } else {
        Severity::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 23, 12, 0, 0).unwrap()
    }

    #[test]
    fn halfway_through_session() {
        // 5h window, 2.5h left -> 50% elapsed.
        let reset = Some(now() + chrono::Duration::minutes(150));
        let p = calc(62, reset, now(), chrono::Duration::hours(5), 5);
        assert_eq!(p.elapsed_pct, 50);
        assert_eq!(p.delta, 12);
        assert_eq!(p.point_pace, Pace::Ahead);
        assert_eq!(p.point_label, "12pts acima");
        assert_eq!(p.ratio_pace, Pace::Ahead);
        assert_eq!(p.ratio_label, "24% acima");

        let p = calc(52, reset, now(), chrono::Duration::hours(5), 5);
        assert_eq!(p.ratio_pace, Pace::OnTrack);
        assert_eq!(p.point_pace, Pace::Ahead);

        let p = calc(10, reset, now(), chrono::Duration::hours(5), 5);
        assert_eq!(p.ratio_pace, Pace::Under);
        assert_eq!(p.point_label, "40pts abaixo");
    }

    #[test]
    fn no_reset_or_zero_window_is_neutral() {
        assert_eq!(
            calc(50, None, now(), chrono::Duration::hours(5), 5),
            Pacing::neutral()
        );
        assert_eq!(
            calc(50, Some(now()), now(), chrono::Duration::zero(), 5),
            Pacing::neutral()
        );
    }

    #[test]
    fn elapsed_is_clamped() {
        let far = Some(now() + chrono::Duration::hours(10));
        assert_eq!(
            calc(0, far, now(), chrono::Duration::hours(5), 5).elapsed_pct,
            0
        );
        let past = Some(now() - chrono::Duration::hours(1));
        assert_eq!(
            calc(0, past, now(), chrono::Duration::hours(5), 5).elapsed_pct,
            100
        );
    }

    #[test]
    fn pace_severity_bands() {
        assert_eq!(pace_severity(10), Severity::Critical);
        assert_eq!(pace_severity(1), Severity::High);
        assert_eq!(pace_severity(0), Severity::Mid);
        assert_eq!(pace_severity(-10), Severity::Mid);
        assert_eq!(pace_severity(-11), Severity::Low);
    }
}
