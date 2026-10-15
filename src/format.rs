//! Small string helpers: `{placeholder}` substitution, money, clock times.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};

use crate::usage::{ResetCredit, ResetCredits};

/// Single-pass `{key}` substitution. Unknown keys are left literal, and
/// substituted values are never re-scanned.
pub fn substitute(template: &str, values: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after
            .find('}')
            .and_then(|close| values.get(&after[..close]).map(|value| (close, value)))
        {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn clamp_pct(v: f64) -> u16 {
    if v.is_nan() {
        0
    } else {
        v.round().clamp(0.0, 100.0) as u16
    }
}

pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn with_currency(sign: &str, number: &str, currency: Option<&str>) -> String {
    match currency {
        None | Some("USD") => format!("{sign}${number}"),
        Some("BRL") => format!("{sign}R${number}"),
        Some("EUR") => format!("{sign}€{number}"),
        Some("GBP") => format!("{sign}£{number}"),
        Some("JPY") | Some("CNY") => format!("{sign}¥{number}"),
        Some(other) => format!("{sign}{number} {other}"),
    }
}

/// Integer minor units to a major-unit string: `250, 2, USD` -> `$2.50`.
pub fn fmt_minor(minor: i64, decimal_places: u32, currency: Option<&str>) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    let number = if decimal_places == 0 {
        abs.to_string()
    } else {
        let scale = 10_u64.pow(decimal_places);
        format!(
            "{}.{:0width$}",
            abs / scale,
            abs % scale,
            width = decimal_places as usize
        )
    };
    with_currency(sign, &number, currency)
}

pub fn local_time_hm(when: DateTime<Utc>) -> String {
    when.with_timezone(&Local).format("%H:%M").to_string()
}

pub fn local_date_hm(when: DateTime<Utc>) -> String {
    when.with_timezone(&Local).format("%d/%m %H:%M").to_string()
}

/// Local wall-clock time of the payload the user is looking at.
pub fn updated_at_hm(now: DateTime<Utc>, cache_age: Option<Duration>) -> String {
    match cache_age {
        Some(age) => local_time_hm(now - chrono::Duration::from_std(age).unwrap_or_default()),
        None => "—".to_string(),
    }
}

pub fn reset_credits_summary(credits: &ResetCredits) -> String {
    let noun = if credits.available == 1 {
        "reset disponível"
    } else {
        "resets disponíveis"
    };
    format!("{} {noun}", credits.available)
}

/// One line per banked reset, soonest expiry first.
pub fn reset_credit_lines(credits: &ResetCredits, now: DateTime<Utc>) -> Vec<String> {
    let mut items: Vec<&ResetCredit> = credits.credits.iter().collect();
    items.sort_by_key(|c| c.expires_at);
    let mut lines: Vec<String> = items.iter().map(|c| reset_credit_line(c, now)).collect();
    if lines.is_empty() && credits.available > 0 {
        lines.push(reset_credits_summary(credits));
    }
    lines
}

fn reset_credit_line(credit: &ResetCredit, now: DateTime<Utc>) -> String {
    let expiry = match credit.expires_at {
        Some(at) if at <= now => format!("expirou {}", local_date_hm(at)),
        Some(at) => format!(
            "expira {} ({})",
            local_date_hm(at),
            crate::countdown::format(Some(at), now)
        ),
        None => "sem data de expiração".into(),
    };
    match credit.title.as_deref() {
        Some(title) => format!("{title}: {expiry}"),
        None => expiry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn substitute_known_unknown_and_unclosed() {
        let v = map(&[("a", "1"), ("b", "{a}")]);
        assert_eq!(substitute("{a}-{b}-{c}-{", &v), "1-{a}-{c}-{");
        assert_eq!(substitute("x{{a}}", &v), "x{1}");
        assert_eq!(substitute("", &v), "");
    }

    #[test]
    fn money() {
        assert_eq!(fmt_minor(250, 2, Some("USD")), "$2.50");
        assert_eq!(fmt_minor(5, 2, Some("BRL")), "R$0.05");
        assert_eq!(fmt_minor(1200, 0, Some("JPY")), "¥1200");
        assert_eq!(fmt_minor(1234, 3, Some("KWD")), "1.234 KWD");
    }

    #[test]
    fn clamp_and_capitalize() {
        assert_eq!(clamp_pct(62.4), 62);
        assert_eq!(clamp_pct(100.6), 100);
        assert_eq!(clamp_pct(f64::NAN), 0);
        assert_eq!(capitalize("pro"), "Pro");
        assert_eq!(capitalize(""), "");
    }
}
