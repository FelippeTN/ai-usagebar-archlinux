//! Snapshot -> Waybar text and bordered Pango tooltip. Pure functions over
//! [`RenderInput`], so everything here is testable without I/O.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::fetch::Outcome;
use crate::format::{reset_credit_lines, reset_credits_summary, substitute, updated_at_hm};
use crate::pacing::{self, Pacing};
use crate::pango::{self, color_span, escape, pct_color};
use crate::sessions::SessionActivity;
use crate::theme::Theme;
use crate::tooltip::{self, Line};
use crate::usage::UsageWindow;
use crate::waybar::WaybarOutput;

pub const DEFAULT_FORMAT: &str = "{session_pct}% · {session_reset}";
pub const DEFAULT_ICON: &str = "󰚩";

pub struct RenderInput<'a> {
    pub outcome: &'a Outcome,
    pub theme: &'a Theme,
    pub format: &'a str,
    pub tooltip_format: Option<&'a str>,
    pub icon: Option<&'a str>,
    pub pace_tolerance: u32,
    /// Color `{*_pace*}` placeholders by pace instead of the whole text by
    /// usage.
    pub format_pace_color: bool,
    /// Tooltip shows point-delta glyphs and an elapsed marker in each bar.
    pub tooltip_pace_pts: bool,
    pub now: DateTime<Utc>,
    pub sessions: Option<SessionActivity>,
}

pub fn render(input: &RenderInput) -> WaybarOutput {
    let snap = &input.outcome.snapshot;
    let severity = snap.severity();
    let values = placeholders(input);

    let mut text = substitute(input.format, &values);
    if input.outcome.stale {
        text.push_str(" ⏸");
    }
    if let Some(icon) = input.icon.filter(|i| !i.is_empty()) {
        text = format!("{} {text}", escape(icon));
    }
    let wrapper = if input.format_pace_color && input.format.contains("_pace") {
        &input.theme.fg
    } else {
        pango::severity_color(severity, input.theme)
    };
    let text = color_span(wrapper, &text);

    let tooltip = match input.tooltip_format {
        Some(fmt) => substitute(fmt, &values),
        None => default_tooltip(input),
    };
    WaybarOutput::new(text, tooltip, severity, input.outcome.stale)
}

fn pace_of(w: &UsageWindow, input: &RenderInput) -> Pacing {
    pacing::calc(
        w.utilization_pct,
        w.resets_at,
        input.now,
        w.window_duration,
        input.pace_tolerance,
    )
}

/// The `{…}` surface for `--format` / `--tooltip-format`. Every key always
/// exists (neutral when a window is absent) so templates never show braces.
fn placeholders(input: &RenderInput) -> HashMap<String, String> {
    let snap = &input.outcome.snapshot;
    let theme = input.theme;
    let mut v: HashMap<String, String> = HashMap::new();
    v.insert("icon".into(), DEFAULT_ICON.into());
    v.insert("plan".into(), escape(&snap.plan));

    let scoped0 = snap.scoped.first();
    let windows: [(&str, Option<&UsageWindow>); 4] = [
        ("session", Some(&snap.session)),
        ("weekly", Some(&snap.weekly)),
        ("sonnet", snap.sonnet.as_ref()),
        ("scoped", scoped0.map(|s| &s.window)),
    ];
    for (prefix, window) in windows {
        let pace = window.map_or_else(Pacing::neutral, |w| pace_of(w, input));
        let key = |suffix: &str| format!("{prefix}_{suffix}");
        let (pct, reset, bar) = match window {
            Some(w) => (
                w.utilization_pct.to_string(),
                countdown::format(w.resets_at, input.now),
                pango::progress_bar(
                    w.utilization_pct,
                    pct_color(w.utilization_pct, theme),
                    theme,
                    None,
                ),
            ),
            None => ("0".into(), "—".into(), String::new()),
        };
        v.insert(key("pct"), pct);
        v.insert(key("reset"), reset);
        v.insert(key("bar"), bar);
        v.insert(key("elapsed"), pace.elapsed_pct.to_string());

        let wrap = |s: &str| {
            if input.format_pace_color {
                color_span(
                    pango::severity_color(pacing::pace_severity(pace.delta), theme),
                    s,
                )
            } else {
                s.to_string()
            }
        };
        v.insert(key("pace"), wrap(pace.ratio_pace.glyph()));
        v.insert(key("pace_indicator"), wrap(pace.point_pace.glyph()));
        v.insert(key("pace_pct"), wrap(&pace.ratio_label));
        v.insert(key("pace_pts"), wrap(&pace.point_label));
        v.insert(key("pace_delta"), wrap(&pace.delta.to_string()));
        v.insert(
            key("pace_abs_delta"),
            wrap(&pace.delta.unsigned_abs().to_string()),
        );
    }
    v.insert(
        "scoped_model".into(),
        scoped0.map(|s| escape(&s.label)).unwrap_or_default(),
    );

    let extra = snap.extra.as_ref();
    v.insert(
        "extra_spent".into(),
        extra.map(|e| escape(&e.fmt_spent())).unwrap_or_default(),
    );
    v.insert(
        "extra_limit".into(),
        extra
            .map(|e| escape(&e.fmt_limit().unwrap_or_else(|| "—".into())))
            .unwrap_or_default(),
    );
    v.insert(
        "extra_pct".into(),
        extra.map_or(0, |e| e.percent()).to_string(),
    );
    v.insert(
        "extra_bar".into(),
        extra
            .map(|e| pango::progress_bar(e.percent(), pct_color(e.percent(), theme), theme, None))
            .unwrap_or_default(),
    );
    v.insert(
        "resets_available".into(),
        snap.reset_credits.available.to_string(),
    );
    v.insert("resets".into(), reset_credits_summary(&snap.reset_credits));
    v
}

/// Icon + label, bar + bold percentage (+ pace glyph), dim reset countdown.
fn push_window(
    lines: &mut Vec<Line>,
    icon: &str,
    label: &str,
    w: &UsageWindow,
    input: &RenderInput,
) {
    let theme = input.theme;
    let pace = pace_of(w, input);
    let color = pct_color(w.utilization_pct, theme);
    let (marker, glyph) = if input.tooltip_pace_pts {
        (Some(pace.elapsed_pct), pace.point_pace.glyph())
    } else {
        (None, pace.ratio_pace.glyph())
    };
    let bar = pango::progress_bar(w.utilization_pct, color, theme, marker);
    let (fg, dim) = (&theme.fg, &theme.dim);
    let pts = if w.resets_at.is_some() {
        format!(" · {}", pace.point_label)
    } else {
        String::new()
    };
    lines.push(Line::Body(format!(
        " <span foreground='{fg}'>  {icon}  {}</span>",
        escape(label)
    )));
    lines.push(Line::Body(format!(
        "   {bar}  <span font_weight='bold' foreground='{color}'>{}% {glyph}</span>",
        w.utilization_pct
    )));
    lines.push(Line::Body(format!(
        " <span foreground='{dim}'>  ⏱  Reseta em {}{pts}</span>",
        escape(&countdown::format(w.resets_at, input.now))
    )));
}

fn default_tooltip(input: &RenderInput) -> String {
    let snap = &input.outcome.snapshot;
    let theme = input.theme;
    let (accent, fg, dim) = (&theme.blue, &theme.fg, &theme.dim);

    let mut lines = vec![
        Line::Center(format!(
            "<span font_weight='bold' foreground='{accent}'>Claude {}</span>",
            escape(&snap.plan)
        )),
        Line::Sep,
        Line::Body(String::new()),
    ];
    push_window(&mut lines, "󰔟", "Sessão (5h)", &snap.session, input);
    lines.push(Line::Body(String::new()));
    push_window(&mut lines, "󰃰", "Semanal", &snap.weekly, input);
    if let Some(w) = &snap.sonnet {
        lines.push(Line::Body(String::new()));
        push_window(&mut lines, "󱤔", "Somente Sonnet", w, input);
    }
    for s in &snap.scoped {
        lines.push(Line::Body(String::new()));
        push_window(
            &mut lines,
            "󰆧",
            &format!("{} semanal", s.label),
            &s.window,
            input,
        );
    }

    if let Some(extra) = &snap.extra {
        let color = pct_color(extra.percent(), theme);
        let bar = pango::progress_bar(extra.percent(), color, theme, None);
        let limit = extra
            .fmt_limit()
            .unwrap_or_else(|| "sem limite informado".into());
        lines.push(Line::Body(String::new()));
        lines.push(Line::Sep);
        lines.push(Line::Body(format!(
            " <span foreground='{fg}'>  󰄑  Uso extra</span>"
        )));
        lines.push(Line::Body(format!(
            "   {bar}  <span font_weight='bold' foreground='{color}'>{}</span>",
            escape(&extra.fmt_spent())
        )));
        lines.push(Line::Body(format!(
            " <span foreground='{dim}'>  󰀓  Limite: {}</span>",
            escape(&limit)
        )));
    }

    if snap.reset_credits.available > 0 {
        lines.push(Line::Body(String::new()));
        lines.push(Line::Sep);
        lines.push(Line::Body(format!(
            " <span foreground='{fg}'>  󰁯  Resets bancados</span>"
        )));
        // Grant labels are long marketing sentences; wrap so they do not
        // stretch the whole tooltip.
        for line in reset_credit_lines(&snap.reset_credits, input.now) {
            for wrapped in wrap_words(&line, 38) {
                lines.push(Line::Body(format!(
                    " <span foreground='{dim}'>     {}</span>",
                    escape(&wrapped)
                )));
            }
        }
    }

    if let Some((code, msg)) = &input.outcome.last_error {
        let (icon, color, title) = match *code {
            0 => ("󰀪", &theme.orange, "Aviso".to_string()),
            c if c >= 500 => ("󰅚", &theme.red, format!("HTTP {c}")),
            c => ("󰀪", &theme.orange, format!("HTTP {c}")),
        };
        lines.push(Line::Body(String::new()));
        lines.push(Line::Sep);
        lines.push(Line::Body(format!(
            " <span foreground='{color}'>  {icon}  {title}</span>"
        )));
        for wrapped in wrap_words(msg, 38) {
            lines.push(Line::Body(format!(
                "     <span foreground='{dim}'>{}</span>",
                escape(&wrapped)
            )));
        }
    }

    if let Some(summary) = input.sessions.and_then(|s| s.summary()) {
        lines.push(Line::Body(String::new()));
        lines.push(Line::Body(format!(
            " <span foreground='{dim}'>  {}</span>",
            escape(&summary)
        )));
    }

    lines.push(Line::Body(String::new()));
    lines.push(Line::Sep);
    lines.push(Line::Body(format!(
        " <span foreground='{dim}'>  󰅐  Atualizado {}</span>",
        updated_at_hm(input.now, input.outcome.cache_age)
    )));
    tooltip::render_bordered(&lines, theme)
}

/// Greedy word wrap, on raw text (escaping happens per line afterwards).
pub fn wrap_words(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for word in s.split_whitespace() {
        if buf.is_empty() {
            buf = word.into();
        } else if buf.chars().count() + 1 + word.chars().count() <= width {
            buf.push(' ');
            buf.push_str(word);
        } else {
            out.push(std::mem::take(&mut buf));
            buf = word.into();
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pango::{strip_markup, visible_width};
    use crate::types::UsageResponse;
    use chrono::TimeZone;
    use std::time::Duration;

    fn outcome(json: &str, stale: bool, last_error: Option<(u16, String)>) -> Outcome {
        let resp: UsageResponse = serde_json::from_str(json).unwrap();
        Outcome {
            snapshot: resp.into_snapshot("Pro".into()),
            stale,
            last_error,
            cache_age: Some(Duration::ZERO),
        }
    }

    fn input<'a>(o: &'a Outcome, theme: &'a Theme, format: &'a str) -> RenderInput<'a> {
        RenderInput {
            outcome: o,
            theme,
            format,
            tooltip_format: None,
            icon: Some(DEFAULT_ICON),
            pace_tolerance: 5,
            format_pace_color: false,
            tooltip_pace_pts: false,
            // 1h30m before the fixture's session reset.
            now: Utc.with_ymd_and_hms(2026, 5, 23, 12, 0, 0).unwrap(),
            sessions: Some(SessionActivity {
                working: 2,
                waiting: 0,
            }),
        }
    }

    const FULL: &str = include_str!("../tests/fixtures/usage_full.json");

    #[test]
    fn bar_text_and_class() {
        let o = outcome(FULL, false, None);
        let t = Theme::default();
        let out = render(&input(&o, &t, DEFAULT_FORMAT));
        assert_eq!(
            out.text,
            format!("<span foreground='{}'>󰚩 62% · 1h 30m</span>", t.yellow)
        );
        assert_eq!(out.class, vec!["mid"]);
    }

    #[test]
    fn stale_suffix_and_class() {
        let o = outcome(FULL, true, Some((0, "Token expirado".into())));
        let t = Theme::default();
        let out = render(&input(&o, &t, DEFAULT_FORMAT));
        assert!(strip_markup(&out.text).ends_with(" ⏸"));
        assert_eq!(out.class, vec!["mid", "stale"]);
        assert!(strip_markup(&out.tooltip).contains("Aviso"));
        assert!(strip_markup(&out.tooltip).contains("Token expirado"));
    }

    #[test]
    fn tooltip_is_a_closed_box_with_every_section() {
        let json = r#"{
            "five_hour":{"utilization":62,"resets_at":"2026-05-23T13:30:00Z"},
            "seven_day":{"utilization":27,"resets_at":"2026-05-27T13:00:00Z"},
            "seven_day_sonnet":{"utilization":4,"resets_at":"2026-05-27T13:00:00Z"},
            "limits":[{"kind":"weekly_scoped","percent":84,"resets_at":"2026-05-27T13:00:00Z",
                       "scope":{"model":{"display_name":"Fable <x>"}}}],
            "extra_usage":{"is_enabled":true,"monthly_limit":null,"used_credits":250,"currency":"USD","decimal_places":2},
            "cedar_ember":{"eligible":true,"grants":[{"label":"Bonus","resets_left":1,"usable_now":true}]}
        }"#;
        let o = outcome(json, false, Some((503, "overloaded".into())));
        let t = Theme::default();
        let out = render(&input(&o, &t, DEFAULT_FORMAT));
        let widths: Vec<usize> = out.tooltip.lines().map(visible_width).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
        let plain = strip_markup(&out.tooltip);
        for needle in [
            "Claude Pro",
            "Sessão (5h)",
            "Semanal",
            "Somente Sonnet",
            "Fable <x> semanal",
            "Uso extra",
            "$2.50",
            "sem limite informado",
            "Resets bancados",
            "Bonus: sem data de expiração",
            "HTTP 503",
            "overloaded",
            "2 trabalhando",
            "Reseta em 1h 30m · 8pts abaixo",
            "Atualizado",
        ] {
            assert!(plain.contains(needle), "missing {needle:?} in\n{plain}");
        }
        // The class follows the worst window (Fable at 84%).
        assert_eq!(out.class, vec!["high"]);
        // Untrusted model name is escaped in the markup.
        assert!(out.tooltip.contains("Fable &lt;x&gt;"));
    }

    #[test]
    fn custom_formats_and_unknown_keys() {
        let o = outcome(FULL, false, None);
        let t = Theme::default();
        let mut i = input(
            &o,
            &t,
            "{plan} S{session_pct} W{weekly_pct} {sonnet_pct} {scoped_pct} {nope}",
        );
        i.icon = None;
        i.tooltip_format = Some("{weekly_reset} {extra_spent}/{extra_limit} {session_pace_pts}");
        let out = render(&i);
        assert_eq!(strip_markup(&out.text), "Pro S62 W27 4 0 {nope}");
        assert_eq!(out.tooltip, "4d 1h $2.50/$50.00 8pts abaixo");
    }

    #[test]
    fn pace_coloring_uses_fg_wrapper() {
        let o = outcome(FULL, false, None);
        let t = Theme::default();
        let mut i = input(&o, &t, "{session_pace_pts}");
        i.format_pace_color = true;
        let out = render(&i);
        assert!(
            out.text
                .starts_with(&format!("<span foreground='{}'>", t.fg))
        );
        // 70% elapsed, 62% used: delta -8 -> mid pace color.
        assert!(out.text.contains(&t.yellow));
    }

    #[test]
    fn wrap() {
        assert_eq!(wrap_words("a bb ccc dddd", 6), vec!["a bb", "ccc", "dddd"]);
        assert!(wrap_words("", 5).is_empty());
    }
}
