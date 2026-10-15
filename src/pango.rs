//! Pango markup helpers: escaping, color spans, progress bars, and the
//! visible width of marked-up text.

use unicode_width::UnicodeWidthChar;

use crate::theme::Theme;
use crate::usage::Severity;

pub const BAR_LEN: u32 = 20;
const FILLED: char = '█';
const EMPTY: char = '░';
const MAX_UNTRUSTED_CHARS: usize = 512;

/// Strip control and bidi-override characters from text that came from the
/// API or a file, and cap its length. Newlines survive.
pub fn sanitize_untrusted(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| match ch {
            '\n' => Some('\n'),
            '\t' | '\r' => Some(' '),
            _ if ch.is_control() || is_bidi_control(ch) => None,
            _ => Some(ch),
        })
        .take(MAX_UNTRUSTED_CHARS)
        .collect()
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Sanitize, then escape `&`, `<`, `>` for Pango.
pub fn escape(s: &str) -> String {
    sanitize_untrusted(s)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `text` must already be Pango-safe.
pub fn color_span(color: &str, text: &str) -> String {
    format!("<span foreground='{color}'>{text}</span>")
}

pub fn severity_color(sev: Severity, theme: &Theme) -> &str {
    match sev {
        Severity::Low => &theme.green,
        Severity::Mid => &theme.yellow,
        Severity::High => &theme.orange,
        Severity::Critical => &theme.red,
    }
}

pub fn pct_color(pct: i32, theme: &Theme) -> &str {
    severity_color(Severity::for_pct(pct), theme)
}

/// A `BAR_LEN`-cell bar. With `marker_pct`, one cell in `theme.marker`
/// marks how much of the window has elapsed.
pub fn progress_bar(pct: i32, fill_color: &str, theme: &Theme, marker_pct: Option<i32>) -> String {
    let filled = (pct.clamp(0, 100) as u32 * BAR_LEN) / 100;
    let empty_color = &theme.bar_empty;

    let Some(marker) = marker_pct.map(|p| p.clamp(0, 100) as u32) else {
        return format!(
            "<span foreground='{fill_color}'>{}</span><span foreground='{empty_color}'>{}</span>",
            repeat(FILLED, filled),
            repeat(EMPTY, BAR_LEN - filled),
        );
    };

    let m = ((marker * BAR_LEN) / 100).min(BAR_LEN - 1);
    let pre_f = filled.min(m);
    let post_f = filled.saturating_sub(m + 1);
    let pre_e = m - pre_f;
    let post_e = BAR_LEN - m - 1 - post_f;
    format!(
        "<span foreground='{fill_color}'>{}</span><span foreground='{empty_color}'>{}</span>\
         <span foreground='{marker_color}'>{FILLED}</span>\
         <span foreground='{fill_color}'>{}</span><span foreground='{empty_color}'>{}</span>",
        repeat(FILLED, pre_f),
        repeat(EMPTY, pre_e),
        repeat(FILLED, post_f),
        repeat(EMPTY, post_e),
        marker_color = theme.marker,
    )
}

fn repeat(c: char, n: u32) -> String {
    std::iter::repeat_n(c, n as usize).collect()
}

/// Terminal-column width with tags stripped and entities counted as one
/// glyph. `width()`, not `width_cjk()`: box-drawing and Nerd Font glyphs are
/// East Asian "Ambiguous" and render one column wide here.
pub fn visible_width(s: &str) -> usize {
    let mut in_tag = false;
    let mut count = 0usize;
    let mut rest = s;
    while let Some(ch) = rest.chars().next() {
        let step = match ch {
            '<' => {
                in_tag = true;
                1
            }
            '>' if in_tag => {
                in_tag = false;
                1
            }
            '&' if !in_tag => {
                count += 1;
                entity_len(rest).unwrap_or(1)
            }
            _ => {
                if !in_tag {
                    count += UnicodeWidthChar::width(ch).unwrap_or(0);
                }
                ch.len_utf8()
            }
        };
        rest = &rest[step..];
    }
    count
}

fn entity_len(s: &str) -> Option<usize> {
    let body = s.strip_prefix('&')?;
    let end = body.find(';')?;
    let name = &body[..end];
    let named = matches!(name, "amp" | "lt" | "gt" | "quot" | "apos");
    let numeric = name
        .strip_prefix("#x")
        .map(|d| u32::from_str_radix(d, 16))
        .or_else(|| name.strip_prefix('#').map(|d| d.parse::<u32>()))
        .is_some_and(|r| r.is_ok());
    (named || numeric).then_some(end + 2)
}

/// Markup to plain text, for `--pretty` terminal output.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_and_sanitize() {
        assert_eq!(escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(escape("x\u{202E}y\u{0007}z\tw"), "xyz w");
    }

    #[test]
    fn bar_has_fixed_width() {
        let t = Theme::default();
        for pct in [0, 1, 50, 99, 100, 150, -5] {
            let bar = progress_bar(pct, "#fff", &t, None);
            assert_eq!(visible_width(&bar), BAR_LEN as usize, "pct {pct}");
            for marker in [0, 50, 100] {
                let bar = progress_bar(pct, "#fff", &t, Some(marker));
                assert_eq!(visible_width(&bar), BAR_LEN as usize, "pct {pct} m {marker}");
            }
        }
        let half = strip_markup(&progress_bar(50, "#fff", &t, None));
        assert_eq!(half.matches(FILLED).count(), 10);
    }

    #[test]
    fn width_ignores_tags_and_counts_entities_once() {
        assert_eq!(visible_width("<span foreground='#fff'>ab</span>"), 2);
        assert_eq!(visible_width("a&amp;b&lt;"), 4);
        assert_eq!(visible_width("a & b"), 5);
        assert_eq!(visible_width("日本"), 4);
        assert_eq!(visible_width("╭─󰔟"), 3);
    }

    #[test]
    fn strip_markup_round_trips_escape() {
        assert_eq!(strip_markup(&color_span("#fff", &escape("1 < 2 & 3"))), "1 < 2 & 3");
    }
}
