//! Wire types for `GET /api/oauth/usage`.
//!
//! The endpoint is undocumented and its shape varies by plan and over time,
//! so every field is optional or defaulted. Values that would be rendered
//! (percentages, currency, scale) are validated here, at the parse boundary:
//! a bad value fails the whole response as `⚠` instead of drawing a number
//! the API never meant.

use serde::{Deserialize, Deserializer, Serialize};

use crate::usage::{
    ClaudeSnapshot, ExtraUsage, ResetCredit, ResetCredits, ScopedWindow, UsageWindow,
};

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct UsageResponse {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
    #[serde(default)]
    pub seven_day_sonnet: Option<Window>,
    #[serde(default)]
    pub extra_usage: Option<ExtraUsageBlock>,
    /// Per-limit array; only `kind == "weekly_scoped"` entries add anything
    /// the named windows above do not already carry.
    #[serde(default, deserialize_with = "de_null_vec")]
    pub limits: Vec<LimitEntry>,
    /// Banked limit resets, present only with `?cedar_ember=1`.
    #[serde(default)]
    pub cedar_ember: Option<ResetGrantsBlock>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct Window {
    #[serde(default, deserialize_with = "de_percent")]
    pub utilization: f64,
    #[serde(default)]
    pub resets_at: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct LimitEntry {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "de_percent_opt")]
    pub percent: Option<f64>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub scope: Option<LimitScope>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct LimitScope {
    #[serde(default)]
    pub model: Option<LimitModel>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct LimitModel {
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExtraUsageBlock {
    #[serde(default)]
    pub is_enabled: bool,
    #[serde(default, deserialize_with = "de_opt_minor")]
    pub monthly_limit: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_minor")]
    pub used_credits: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_currency")]
    pub currency: Option<String>,
    #[serde(default, deserialize_with = "de_opt_decimal_places")]
    pub decimal_places: Option<u32>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct ResetGrantsBlock {
    #[serde(default)]
    pub eligible: bool,
    #[serde(default, deserialize_with = "de_null_vec")]
    pub grants: Vec<ResetGrant>,
}

/// The grant `id` (the handle that spends a reset) is never deserialized.
#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
pub struct ResetGrant {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub resets_left: u32,
    #[serde(default)]
    pub ends_at: Option<String>,
    #[serde(default)]
    pub usable_now: bool,
    #[serde(default)]
    pub paused: bool,
}

/// The endpoint occasionally reports a hair over 100; that slack is
/// accepted and clamped, anything further is drift.
const PCT_SLACK: f64 = 1.0;

fn checked_percent<E: serde::de::Error>(v: f64) -> Result<f64, E> {
    if !v.is_finite() {
        return Err(E::custom(format!("percentage {v} is not finite")));
    }
    if !(0.0..=100.0 + PCT_SLACK).contains(&v) {
        return Err(E::custom(format!("percentage {v} outside 0..=100")));
    }
    Ok(v)
}

fn de_percent<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    match Option::<f64>::deserialize(d)? {
        Some(v) => checked_percent(v),
        None => Ok(0.0),
    }
}

fn de_percent_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(d)?
        .map(checked_percent)
        .transpose()
}

fn de_null_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// Non-negative integer minor units; integral floats (`250.0`) are accepted
/// because the endpoint emits them.
fn de_opt_minor<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    const MAX_EXACT_F64_INT: f64 = (1_u64 << 53) as f64;
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= 0 {
                    return Ok(Some(i));
                }
            } else if let Some(f) = n.as_f64()
                && f.is_finite()
                && f.fract() == 0.0
                && (0.0..=MAX_EXACT_F64_INT).contains(&f)
            {
                return Ok(Some(f as i64));
            }
            Err(serde::de::Error::custom(
                "amount must be a non-negative integer",
            ))
        }
        _ => Err(serde::de::Error::custom("amount must be a number or null")),
    }
}

/// Three uppercase ASCII letters: the value lands in Pango markup verbatim.
fn de_opt_currency<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Option::<String>::deserialize(d)? {
        None => Ok(None),
        Some(s) if s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase()) => Ok(Some(s)),
        Some(s) => Err(serde::de::Error::custom(format!(
            "currency {s:?} is not an ISO 4217 code"
        ))),
    }
}

fn de_opt_decimal_places<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let n = match serde_json::Value::deserialize(d)? {
        serde_json::Value::Null => return Ok(None),
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        _ => None,
    };
    match n {
        Some(n) if (0..=6).contains(&n) => Ok(Some(n as u32)),
        _ => Err(serde::de::Error::custom(
            "decimal_places must be an integer in 0..=6",
        )),
    }
}

fn parse_time(s: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    s.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

fn to_window(w: Option<Window>, dur: chrono::Duration) -> UsageWindow {
    match w {
        Some(w) => UsageWindow {
            utilization_pct: i32::from(crate::format::clamp_pct(w.utilization)),
            resets_at: parse_time(w.resets_at.as_deref()),
            window_duration: dur,
        },
        None => UsageWindow {
            utilization_pct: 0,
            resets_at: None,
            window_duration: dur,
        },
    }
}

impl UsageResponse {
    /// `plan` comes from the credentials file; the endpoint does not send it.
    pub fn into_snapshot(self, plan: String) -> ClaudeSnapshot {
        const SESSION: chrono::Duration = chrono::Duration::hours(5);
        const WEEKLY: chrono::Duration = chrono::Duration::days(7);

        let extra = self.extra_usage.filter(|e| e.is_enabled).and_then(|e| {
            Some(ExtraUsage {
                limit: e.monthly_limit,
                // Without the spend there is nothing to show.
                spent: e.used_credits?,
                currency: e.currency,
                decimal_places: e.decimal_places,
            })
        });

        let scoped = self
            .limits
            .into_iter()
            .filter(|l| l.kind.as_deref() == Some("weekly_scoped"))
            .filter_map(|l| {
                let label = crate::pango::sanitize_untrusted(l.scope?.model?.display_name?.trim());
                if label.is_empty() {
                    return None;
                }
                // No percentage = no bar; never default to a confident 0%.
                let utilization = l.percent?;
                let window = to_window(
                    Some(Window {
                        utilization,
                        resets_at: l.resets_at,
                    }),
                    WEEKLY,
                );
                Some(ScopedWindow { label, window })
            })
            .collect();

        ClaudeSnapshot {
            plan,
            session: to_window(self.five_hour, SESSION),
            weekly: to_window(self.seven_day, WEEKLY),
            sonnet: self.seven_day_sonnet.map(|w| to_window(Some(w), WEEKLY)),
            scoped,
            extra,
            reset_credits: reset_credits(self.cedar_ember),
        }
    }
}

/// Only grants the server says are redeemable now are counted.
fn reset_credits(block: Option<ResetGrantsBlock>) -> ResetCredits {
    let Some(block) = block.filter(|b| b.eligible) else {
        return ResetCredits::default();
    };
    let mut credits = ResetCredits::default();
    for grant in block
        .grants
        .into_iter()
        .filter(|g| g.usable_now && !g.paused && g.resets_left > 0)
    {
        credits.available = credits.available.saturating_add(grant.resets_left);
        credits.credits.push(ResetCredit {
            title: grant
                .label
                .map(|t| crate::pango::sanitize_untrusted(t.trim()))
                .filter(|t| !t.is_empty()),
            expires_at: parse_time(grant.ends_at.as_deref()),
        });
    }
    credits
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse(json: &str) -> serde_json::Result<UsageResponse> {
        serde_json::from_str(json)
    }

    #[test]
    fn full_fixture() {
        let resp = parse(include_str!("../tests/fixtures/usage_full.json")).unwrap();
        let s = resp.into_snapshot("Max 5x".into());
        assert_eq!(s.session.utilization_pct, 62);
        assert_eq!(s.weekly.utilization_pct, 27);
        assert_eq!(s.sonnet.as_ref().unwrap().utilization_pct, 4);
        assert_eq!(
            s.session.resets_at.unwrap().to_rfc3339(),
            "2026-05-23T13:30:00+00:00"
        );
        let extra = s.extra.unwrap();
        assert_eq!(extra.spent, 250);
        assert_eq!(extra.limit, Some(5000));
        assert_eq!(extra.fmt_spent(), "$2.50");
    }

    #[test]
    fn minimal_fixture() {
        let s = parse(include_str!("../tests/fixtures/usage_minimal.json"))
            .unwrap()
            .into_snapshot("Pro".into());
        assert_eq!(s.session.utilization_pct, 15);
        assert!(s.sonnet.is_none());
        assert!(s.extra.is_none());
        assert!(s.scoped.is_empty());
        assert_eq!(s.reset_credits.available, 0);
    }

    #[test]
    fn empty_object_is_all_zero() {
        let s = parse("{}").unwrap().into_snapshot("Pro".into());
        assert_eq!(s.session.utilization_pct, 0);
        assert!(s.session.resets_at.is_none());
    }

    #[test]
    fn percent_slack_is_clamped_and_drift_rejected() {
        let s = parse(r#"{"five_hour":{"utilization":100.6}}"#)
            .unwrap()
            .into_snapshot("Pro".into());
        assert_eq!(s.session.utilization_pct, 100);
        assert!(parse(r#"{"five_hour":{"utilization":250}}"#).is_err());
        assert!(parse(r#"{"five_hour":{"utilization":-1}}"#).is_err());
        assert!(parse(r#"{"five_hour":{"utilization":null}}"#).is_ok());
    }

    #[test]
    fn scoped_limits_need_label_and_percent() {
        let json = r#"{"limits":[
            {"kind":"weekly_scoped","percent":84,"resets_at":"2026-05-27T13:00:00Z",
             "scope":{"model":{"id":null,"display_name":"Fable"}}},
            {"kind":"weekly_scoped","scope":{"model":{"display_name":"NoPct"}}},
            {"kind":"weekly_scoped","percent":10,"scope":{"model":null}},
            {"kind":"session","percent":50}
        ]}"#;
        let s = parse(json).unwrap().into_snapshot("Pro".into());
        assert_eq!(s.scoped.len(), 1);
        assert_eq!(s.scoped[0].label, "Fable");
        assert_eq!(s.scoped[0].window.utilization_pct, 84);
    }

    #[test]
    fn extra_usage_validation() {
        assert!(
            parse(r#"{"extra_usage":{"is_enabled":true,"used_credits":1,"currency":"<b>"}}"#)
                .is_err()
        );
        assert!(
            parse(r#"{"extra_usage":{"is_enabled":true,"used_credits":1,"decimal_places":9}}"#)
                .is_err()
        );
        assert!(parse(r#"{"extra_usage":{"is_enabled":true,"used_credits":-5}}"#).is_err());
        let s = parse(r#"{"extra_usage":{"is_enabled":true,"monthly_limit":null,"used_credits":14157.0,"currency":"USD","decimal_places":2}}"#)
            .unwrap()
            .into_snapshot("Pro".into());
        let e = s.extra.unwrap();
        assert_eq!(e.limit, None);
        assert_eq!(e.fmt_spent(), "$141.57");
        let s = parse(r#"{"extra_usage":{"is_enabled":false,"used_credits":1}}"#)
            .unwrap()
            .into_snapshot("Pro".into());
        assert!(s.extra.is_none());
    }

    #[test]
    fn reset_grants_only_count_usable_ones() {
        let json = r#"{"cedar_ember":{"eligible":true,"grants":[
            {"id":"secret","label":"Launch reset","resets_left":2,"usable_now":true,"ends_at":"2026-06-01T00:00:00Z"},
            {"resets_left":1,"usable_now":false},
            {"resets_left":1,"usable_now":true,"paused":true},
            {"resets_left":0,"usable_now":true}
        ]}}"#;
        let s = parse(json).unwrap().into_snapshot("Pro".into());
        assert_eq!(s.reset_credits.available, 2);
        assert_eq!(s.reset_credits.credits.len(), 1);
        assert_eq!(
            s.reset_credits.credits[0].title.as_deref(),
            Some("Launch reset")
        );
        let s = parse(r#"{"cedar_ember":{"eligible":false,"ineligible_reason":"surface"}}"#)
            .unwrap()
            .into_snapshot("Pro".into());
        assert_eq!(s.reset_credits.available, 0);
    }
}
