// src/tui/widgets/costs.rs — Token and cost analytics panel (Tab 4).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::tui::data::CostsData;
use crate::tui::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, data: &CostsData) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Percentage(45),
            Constraint::Min(10),
        ])
        .split(area);

    render_summary(f, chunks[0], data);
    render_model_breakdown(f, chunks[1], data);
    render_daily_costs(f, chunks[2], data);
}

fn render_summary(f: &mut Frame, area: Rect, data: &CostsData) {
    let block = Block::default()
        .title(" Cost Summary ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let lines = vec![
        Line::from(vec![
            Span::styled("  Total tokens: ", Theme::text_dim()),
            Span::styled(format_tokens(data.total_tokens), Theme::text()),
            Span::styled("    Total cost: ", Theme::text_dim()),
            Span::styled(
                format!("${:.4}", data.total_cost_usd),
                if data.total_cost_usd > 1.0 {
                    Theme::warning()
                } else {
                    Theme::success()
                },
            ),
            Span::styled(
                format!("    Models: {}", data.by_model.len()),
                Theme::text_dim(),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Days tracked: ", Theme::text_dim()),
            Span::styled(data.daily.len().to_string(), Theme::text()),
        ]),
    ];

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_model_breakdown(f: &mut Frame, area: Rect, data: &CostsData) {
    let block = Block::default()
        .title(format!(" Cost by Model ({}) ", data.by_model.len()))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.by_model.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No model usage data yet.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Provider").style(Theme::table_header()),
        Cell::from("Model").style(Theme::table_header()),
        Cell::from("Tokens").style(Theme::table_header()),
        Cell::from("Cost").style(Theme::table_header()),
        Cell::from("Sessions").style(Theme::table_header()),
        Cell::from("% of Total").style(Theme::table_header()),
    ]);

    let rows: Vec<Row> = data
        .by_model
        .iter()
        .map(|m| {
            let pct = if data.total_cost_usd > 0.0 {
                (m.cost / data.total_cost_usd) * 100.0
            } else {
                0.0
            };
            Row::new(vec![
                Cell::from(m.provider.clone()).style(Theme::info()),
                Cell::from(m.model.clone()).style(Theme::text()),
                Cell::from(format_tokens(m.tokens)).style(Theme::text_dim()),
                Cell::from(format!("${:.4}", m.cost)).style(Theme::warning()),
                Cell::from(m.sessions.to_string()).style(Theme::text_dim()),
                Cell::from(format!("{:.1}%", pct)).style(Theme::text()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths).header(header).block(block);
    f.render_widget(table, area);
}

fn render_daily_costs(f: &mut Frame, area: Rect, data: &CostsData) {
    let block = Block::default()
        .title(" Daily Cost (last 30 days) ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.daily.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No daily cost data yet.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    // Simple text-based sparkline / bar chart
    let max_cost = data.daily.iter().map(|d| d.cost).fold(0.0_f64, f64::max);

    let bar_width = area.width.saturating_sub(30) as usize;

    let lines: Vec<Line> = data
        .daily
        .iter()
        .take(area.height.saturating_sub(2) as usize)
        .map(|d| {
            let bar_len = if max_cost > 0.0 {
                ((d.cost / max_cost) * bar_width as f64) as usize
            } else {
                0
            };
            let bar: String = "\u{2588}".repeat(bar_len);

            Line::from(vec![
                Span::styled(format!("  {} ", d.day), Theme::text_dim()),
                Span::styled(format!("${:.4} ", d.cost), Theme::warning()),
                Span::styled(bar, Theme::info()),
                Span::styled(
                    format!(" {}tk {}t", format_tokens(d.tokens), d.tasks),
                    Theme::text_dim(),
                ),
            ])
        })
        .collect();

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
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
