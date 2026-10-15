use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Gauge, Paragraph, Wrap};

use crate::countdown;
use crate::format::{reset_credit_lines, updated_at_hm};
use crate::pacing;
use crate::theme::Theme;
use crate::usage::{Severity, UsageWindow};

use super::App;

struct Palette {
    accent: Color,
    fg: Color,
    dim: Color,
    empty: Color,
    theme: Theme,
}

impl Palette {
    fn new(theme: &Theme) -> Self {
        Self {
            accent: hex(&theme.blue),
            fg: hex(&theme.fg),
            dim: hex(&theme.dim),
            empty: hex(&theme.bar_empty),
            theme: theme.clone(),
        }
    }

    fn severity(&self, pct: i32) -> Color {
        let t = &self.theme;
        hex(match Severity::for_pct(pct) {
            Severity::Low => &t.green,
            Severity::Mid => &t.yellow,
            Severity::High => &t.orange,
            Severity::Critical => &t.red,
        })
    }
}

fn hex(s: &str) -> Color {
    let h = s.trim_start_matches('#');
    let byte = |i: usize| h.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    match (byte(0), byte(2), byte(4)) {
        (Some(r), Some(g), Some(b)) => Color::Rgb(r, g, b),
        _ => Color::Reset,
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let p = Palette::new(&app.settings.theme);
    let plan = app
        .outcome
        .as_ref()
        .map(|o| o.snapshot.plan.as_str())
        .unwrap_or("…");

    let status = if app.refreshing {
        Span::styled(" atualizando… ", Style::new().fg(p.accent))
    } else {
        Span::styled(
            format!(" próxima em {}s ", app.next_refresh_in().as_secs()),
            Style::new().fg(p.dim),
        )
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(p.accent))
        .title(Span::styled(
            format!(" 󰚩 Claude {plan} "),
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
        ))
        .title(Line::from(status).right_aligned())
        .title_bottom(Line::from(Span::styled(
            " r atualizar · q sair ",
            Style::new().fg(p.dim),
        )));
    let area = block.inner(f.area());
    f.render_widget(block, f.area());

    let Some(outcome) = &app.outcome else {
        let msg = app
            .error
            .clone()
            .unwrap_or_else(|| "Carregando…".to_string());
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::new().fg(p.fg))
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Center),
            pad(area),
        );
        return;
    };
    let snap = &outcome.snapshot;
    let now = Utc::now();

    let windows = snap.windows();
    let mut constraints: Vec<Constraint> = windows.iter().map(|_| Constraint::Length(4)).collect();
    let extra_rows = if snap.extra.is_some() { 4 } else { 0 };
    let credit_lines = reset_credit_lines(&snap.reset_credits, now);
    let credit_rows = if credit_lines.is_empty() {
        0
    } else {
        credit_lines.len() as u16 + 2
    };
    constraints.push(Constraint::Length(extra_rows));
    constraints.push(Constraint::Length(credit_rows));
    constraints.push(Constraint::Min(0));
    let rows = Layout::vertical(constraints).split(pad(area));

    let icons = ["󰔟", "󰃰", "󱤔"];
    for (i, (label, w)) in windows.iter().enumerate() {
        let icon = icons.get(i).copied().unwrap_or("󰆧");
        draw_window(f, rows[i], &p, icon, label, w, app.settings.pace_tolerance);
    }

    let mut next = windows.len();
    if let Some(extra) = &snap.extra {
        let limit = extra
            .fmt_limit()
            .unwrap_or_else(|| "sem limite informado".into());
        draw_meter(
            f,
            rows[next],
            &p,
            "󰄑",
            "Uso extra",
            extra.percent(),
            extra.fmt_spent(),
            format!("Limite: {limit}"),
        );
    }
    next += 1;

    if !credit_lines.is_empty() {
        let mut lines = vec![Line::styled("󰁯  Resets bancados", Style::new().fg(p.fg))];
        lines.extend(
            credit_lines
                .into_iter()
                .map(|l| Line::styled(format!("   {l}"), Style::new().fg(p.dim))),
        );
        f.render_widget(Paragraph::new(lines), rows[next]);
    }
    next += 1;

    let mut notes: Vec<Line> = Vec::new();
    if let Some(summary) = app.sessions.summary() {
        notes.push(Line::styled(summary, Style::new().fg(p.fg)));
    }
    if let Some((code, msg)) = &outcome.last_error {
        let title = if *code == 0 {
            "Aviso".to_string()
        } else {
            format!("HTTP {code}")
        };
        notes.push(Line::styled(
            format!("󰀪  {title}: {msg}"),
            Style::new().fg(hex(&p.theme.orange)),
        ));
    }
    if let Some(err) = &app.error {
        notes.push(Line::styled(
            format!("󰅚  {}", err.replace('\n', " ")),
            Style::new().fg(hex(&p.theme.red)),
        ));
    }
    let freshness = if outcome.stale {
        "⏸ dados do cache"
    } else {
        "● ao vivo"
    };
    notes.push(Line::styled(
        format!(
            "󰅐  Atualizado {} · {freshness}",
            updated_at_hm(now, outcome.cache_age)
        ),
        Style::new().fg(p.dim),
    ));
    f.render_widget(Paragraph::new(notes).wrap(Wrap { trim: true }), rows[next]);
}

fn pad(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    }
}

fn draw_window(
    f: &mut Frame,
    area: Rect,
    p: &Palette,
    icon: &str,
    label: &str,
    w: &UsageWindow,
    tolerance: u32,
) {
    let now = Utc::now();
    let pace = pacing::calc(
        w.utilization_pct,
        w.resets_at,
        now,
        w.window_duration,
        tolerance,
    );
    let mut detail = format!("Reseta em {}", countdown::format(w.resets_at, now));
    if w.resets_at.is_some() {
        detail.push_str(&format!(
            " · {} · {}% do tempo decorrido",
            pace.point_label, pace.elapsed_pct
        ));
    }
    draw_meter(
        f,
        area,
        p,
        icon,
        label,
        w.utilization_pct,
        format!("{}% {}", w.utilization_pct, pace.ratio_pace.glyph()),
        detail,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_meter(
    f: &mut Frame,
    area: Rect,
    p: &Palette,
    icon: &str,
    label: &str,
    pct: i32,
    value: String,
    detail: String,
) {
    let [head, gauge, foot, _] = Layout::vertical([Constraint::Length(1); 4]).areas(area);
    let color = p.severity(pct);
    f.render_widget(
        Paragraph::new(format!("{icon}  {label}")).style(Style::new().fg(p.fg)),
        head,
    );
    f.render_widget(
        Paragraph::new(value)
            .style(Style::new().fg(color).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Right),
        head,
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::new().fg(color).bg(p.empty))
            .percent(pct.clamp(0, 100) as u16)
            .label("")
            .use_unicode(true),
        gauge,
    );
    f.render_widget(Paragraph::new(detail).style(Style::new().fg(p.dim)), foot);
}
