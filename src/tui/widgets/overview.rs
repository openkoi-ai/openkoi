// src/tui/widgets/overview.rs — System overview panel (Tab 1).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::data::OverviewData;
use crate::tui::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, data: &OverviewData) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_system_info(f, chunks[0], data);
    render_stats(f, chunks[1], data);
}

fn render_system_info(f: &mut Frame, area: Rect, data: &OverviewData) {
    let block = Block::default()
        .title(" System ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let daemon_status = if data.daemon_running {
        Span::styled("running", Theme::success())
    } else {
        Span::styled("stopped", Theme::text_dim())
    };

    let integrations_text = if data.integrations.is_empty() {
        "none".to_string()
    } else {
        data.integrations.join(", ")
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Version:       ", Theme::text_dim()),
            Span::styled(format!("v{}", data.version), Theme::text()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Daemon:        ", Theme::text_dim()),
            daemon_status,
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Plugins:       ", Theme::text_dim()),
            Span::styled(&data.plugin_summary, Theme::text()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Integrations:  ", Theme::text_dim()),
            Span::styled(
                format!("{} ({})", data.integration_count, integrations_text),
                Theme::text(),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_stats(f: &mut Frame, area: Rect, data: &OverviewData) {
    let block = Block::default()
        .title(" Totals ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let score_style = Theme::score(data.avg_score);

    let lines = vec![
        Line::from(vec![
            Span::styled("Sessions:      ", Theme::text_dim()),
            Span::styled(data.total_sessions.to_string(), Theme::text()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tasks:         ", Theme::text_dim()),
            Span::styled(data.total_tasks.to_string(), Theme::text()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Learnings:     ", Theme::text_dim()),
            Span::styled(data.total_learnings.to_string(), Theme::info()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Patterns:      ", Theme::text_dim()),
            Span::styled(data.total_patterns.to_string(), Theme::text()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Avg score:     ", Theme::text_dim()),
            Span::styled(format!("{:.2}", data.avg_score), score_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Total tokens:  ", Theme::text_dim()),
            Span::styled(format_tokens(data.total_tokens), Theme::text()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Total cost:    ", Theme::text_dim()),
            Span::styled(format!("${:.4}", data.total_cost_usd), Theme::warning()),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
