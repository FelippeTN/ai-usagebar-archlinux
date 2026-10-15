//! The bordered Pango tooltip box (`╭─╮ │ ╰─╯`), auto-sized to its widest
//! line.

use crate::pango::visible_width;
use crate::theme::Theme;

pub enum Line {
    Center(String),
    Body(String),
    Sep,
}

pub fn render_bordered(lines: &[Line], theme: &Theme) -> String {
    let border = &theme.blue;
    let dim = &theme.dim;

    let max_w = lines
        .iter()
        .filter_map(|l| match l {
            Line::Center(s) | Line::Body(s) => Some(visible_width(s)),
            Line::Sep => None,
        })
        .max()
        .unwrap_or(0);
    let inner_w = max_w + 1;
    let border_h = "─".repeat(inner_w);
    let sep_line = format!(
        " <span foreground='{dim}'>{}</span>",
        "─".repeat(inner_w.saturating_sub(2))
    );

    let mut out = String::with_capacity(256 * lines.len());
    out.push_str(&format!("<span foreground='{border}'>╭{border_h}╮</span>\n"));
    for line in lines {
        let body = match line {
            Line::Body(s) => pad_right(s, inner_w),
            Line::Center(s) => pad_center(s, inner_w),
            Line::Sep => pad_right(&sep_line, inner_w),
        };
        out.push_str(&format!(
            "<span foreground='{border}'>│</span>{body}<span foreground='{border}'>│</span>\n"
        ));
    }
    out.push_str(&format!("<span foreground='{border}'>╰{border_h}╯</span>"));
    out
}

fn pad_right(s: &str, inner_w: usize) -> String {
    let need = inner_w.saturating_sub(visible_width(s));
    format!("{s}{}", " ".repeat(need))
}

/// Odd leftover space goes on the right.
fn pad_center(s: &str, inner_w: usize) -> String {
    let total = inner_w.saturating_sub(visible_width(s));
    let left = total / 2;
    format!("{}{s}{}", " ".repeat(left), " ".repeat(total - left))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_the_same_width() {
        let lines = vec![
            Line::Center("<b>Título</b>".into()),
            Line::Sep,
            Line::Body(" curto".into()),
            Line::Body(" uma linha bem mais comprida &amp; larga".into()),
        ];
        let out = render_bordered(&lines, &Theme::default());
        let widths: Vec<usize> = out.lines().map(visible_width).collect();
        assert_eq!(widths.len(), 6);
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
    }
}
