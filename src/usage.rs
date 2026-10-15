//! Canonical snapshot of a Claude account's limits, independent of the wire
//! format.

use chrono::{DateTime, Utc};

/// Least to most severe; the derived `Ord` makes `max` mean "worse".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Mid,
    High,
    Critical,
}

impl Severity {
    /// >= 90 critical, >= 75 high, >= 50 mid, else low.
    pub fn for_pct(pct: i32) -> Self {
        if pct >= 90 {
            Self::Critical
        } else if pct >= 75 {
            Self::High
        } else if pct >= 50 {
            Self::Mid
        } else {
            Self::Low
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Mid => "mid",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindow {
    pub utilization_pct: i32,
    pub resets_at: Option<DateTime<Utc>>,
    pub window_duration: chrono::Duration,
}

/// A per-model weekly cap from `limits[]` (e.g. "Fable").
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedWindow {
    pub label: String,
    pub window: UsageWindow,
}

/// Pay-as-you-go spend. Amounts are integer minor units.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraUsage {
    /// `None` = no cap reported (uncapped plans send `null`).
    pub limit: Option<i64>,
    pub spent: i64,
    pub currency: Option<String>,
    pub decimal_places: Option<u32>,
}

impl ExtraUsage {
    pub fn percent(&self) -> i32 {
        match self.limit {
            Some(l) if l > 0 => ((self.spent.saturating_mul(100)) / l).clamp(0, 999) as i32,
            _ => 0,
        }
    }

    pub fn fmt_spent(&self) -> String {
        self.fmt_amount(self.spent)
    }

    pub fn fmt_limit(&self) -> Option<String> {
        self.limit.map(|l| self.fmt_amount(l))
    }

    fn fmt_amount(&self, amount: i64) -> String {
        match (self.decimal_places, self.currency.as_deref()) {
            (Some(dp), currency) => crate::format::fmt_minor(amount, dp, currency),
            // Legacy payloads had neither field and were always USD cents.
            (None, None) => crate::format::fmt_minor(amount, 2, None),
            // A currency alone does not tell the exponent; do not guess.
            (None, Some(currency)) => format!("{amount} minor units {currency}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResetCredit {
    pub title: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Banked limit resets (`cedar_ember`), only the ones usable right now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResetCredits {
    pub available: u32,
    pub credits: Vec<ResetCredit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSnapshot {
    pub plan: String,
    pub session: UsageWindow,
    pub weekly: UsageWindow,
    pub sonnet: Option<UsageWindow>,
    pub scoped: Vec<ScopedWindow>,
    pub extra: Option<ExtraUsage>,
    pub reset_credits: ResetCredits,
}

impl ClaudeSnapshot {
    /// The worst window drives the Waybar class. Extra-usage spend only
    /// counts once some window is capped, since that is when it is spent.
    pub fn severity(&self) -> Severity {
        let windows = self.windows();
        let mut max = windows
            .iter()
            .map(|(_, w)| w.utilization_pct)
            .max()
            .unwrap_or(0);
        let any_at_cap = windows.iter().any(|(_, w)| w.utilization_pct >= 100);
        if any_at_cap && let Some(extra) = &self.extra {
            max = max.max(extra.percent());
        }
        Severity::for_pct(max)
    }

    /// Every rate-limit window with a display label, in tooltip order.
    pub fn windows(&self) -> Vec<(String, &UsageWindow)> {
        let mut out = vec![
            ("Sessão (5h)".to_string(), &self.session),
            ("Semanal".to_string(), &self.weekly),
        ];
        if let Some(s) = &self.sonnet {
            out.push(("Somente Sonnet".to_string(), s));
        }
        for s in &self.scoped {
            out.push((format!("{} semanal", s.label), &s.window));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pct: i32) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: None,
            window_duration: chrono::Duration::hours(5),
        }
    }

    fn snap(session: i32, weekly: i32) -> ClaudeSnapshot {
        ClaudeSnapshot {
            plan: "Pro".into(),
            session: window(session),
            weekly: window(weekly),
            sonnet: None,
            scoped: vec![],
            extra: None,
            reset_credits: ResetCredits::default(),
        }
    }

    #[test]
    fn severity_thresholds() {
        assert_eq!(Severity::for_pct(49), Severity::Low);
        assert_eq!(Severity::for_pct(50), Severity::Mid);
        assert_eq!(Severity::for_pct(75), Severity::High);
        assert_eq!(Severity::for_pct(90), Severity::Critical);
    }

    #[test]
    fn worst_window_wins() {
        assert_eq!(snap(10, 80).severity(), Severity::High);
        let mut s = snap(10, 10);
        s.scoped.push(ScopedWindow {
            label: "Fable".into(),
            window: window(95),
        });
        assert_eq!(s.severity(), Severity::Critical);
    }

    #[test]
    fn extra_usage_only_counts_when_capped() {
        let extra = ExtraUsage {
            limit: Some(1000),
            spent: 950,
            currency: Some("USD".into()),
            decimal_places: Some(2),
        };
        let mut s = snap(40, 10);
        s.extra = Some(extra.clone());
        assert_eq!(s.severity(), Severity::Low);
        let mut s = snap(100, 10);
        s.extra = Some(ExtraUsage {
            spent: 100,
            ..extra
        });
        // Capped window already critical; extra at 10% does not lower it.
        assert_eq!(s.severity(), Severity::Critical);
    }

    #[test]
    fn extra_formatting() {
        let e = ExtraUsage {
            limit: Some(5000),
            spent: 250,
            currency: Some("BRL".into()),
            decimal_places: Some(2),
        };
        assert_eq!(e.fmt_spent(), "R$2.50");
        assert_eq!(e.fmt_limit().as_deref(), Some("R$50.00"));
        assert_eq!(e.percent(), 5);
        let legacy = ExtraUsage {
            currency: None,
            decimal_places: None,
            ..e.clone()
        };
        assert_eq!(legacy.fmt_spent(), "$2.50");
        let unknown_scale = ExtraUsage {
            decimal_places: None,
            ..e
        };
        assert_eq!(unknown_scale.fmt_spent(), "250 minor units BRL");
    }
}
