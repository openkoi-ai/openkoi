// src/tui/widgets/tasks.rs — Task history and findings panel (Tab 2).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::tui::data::TasksData;
use crate::tui::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, data: &TasksData, table_state: &mut TableState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    render_task_table(f, chunks[0], data, table_state);
    render_findings(f, chunks[1], data);
}

fn render_task_table(f: &mut Frame, area: Rect, data: &TasksData, state: &mut TableState) {
    let header = Row::new(vec![
        Cell::from("Date").style(Theme::table_header()),
        Cell::from("Description").style(Theme::table_header()),
        Cell::from("Cat").style(Theme::table_header()),
        Cell::from("Score").style(Theme::table_header()),
        Cell::from("Iter").style(Theme::table_header()),
        Cell::from("Decision").style(Theme::table_header()),
        Cell::from("Tokens").style(Theme::table_header()),
        Cell::from("Cost").style(Theme::table_header()),
    ]);

    let rows: Vec<Row> = data
        .tasks
        .iter()
        .map(|t| {
            let score_text = t
                .final_score
                .map(|s| format!("{:.2}", s))
                .unwrap_or_else(|| "-".into());
            let score_style = t.final_score.map(Theme::score).unwrap_or(Theme::text_dim());

            let tokens_text = t
                .total_tokens
                .map(format_tokens)
                .unwrap_or_else(|| "-".into());

            let cost_text = t
                .total_cost
                .map(|c| format!("${:.4}", c))
                .unwrap_or_else(|| "-".into());

            let desc = truncate(&t.description, 40);
            let date = t.created_at.get(..10).unwrap_or(&t.created_at);

            Row::new(vec![
                Cell::from(date.to_string()).style(Theme::text_dim()),
                Cell::from(desc).style(Theme::text()),
                Cell::from(t.category.clone()).style(Theme::text_dim()),
                Cell::from(score_text).style(score_style),
                Cell::from(
                    t.iterations
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "-".into()),
                )
                .style(Theme::text()),
                Cell::from(t.decision.clone()).style(decision_style(&t.decision)),
                Cell::from(tokens_text).style(Theme::text_dim()),
                Cell::from(cost_text).style(Theme::text_dim()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(format!(" Tasks ({}) ", data.tasks.len()))
                .borders(Borders::ALL)
                .border_style(Theme::border()),
        )
        .row_highlight_style(Theme::table_selected())
        .highlight_symbol(">> ");

    f.render_stateful_widget(table, area, state);
}

fn render_findings(f: &mut Frame, area: Rect, data: &TasksData) {
    let block = Block::default()
        .title(format!(
            " Recent Findings ({}) ",
            data.recent_findings.len()
        ))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.recent_findings.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No findings recorded yet.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = data
        .recent_findings
        .iter()
        .take(area.height.saturating_sub(2) as usize)
        .map(|finding| {
            let sev_style = match finding.severity.as_str() {
                "critical" | "error" => Theme::error(),
                "warning" => Theme::warning(),
                "info" => Theme::info(),
                _ => Theme::text_dim(),
            };
            Line::from(vec![
                Span::styled(
                    format!(" {:>8} ", finding.severity),
                    sev_style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("[{}] ", finding.dimension), Theme::text_dim()),
                Span::styled(truncate(&finding.title, 50), Theme::text()),
                Span::styled(
                    format!("  ({})", truncate(&finding.task_desc, 25)),
                    Theme::text_dim(),
                ),
            ])
        })
        .collect();

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

fn decision_style(decision: &str) -> ratatui::style::Style {
    match decision {
        "accept" | "accepted" => Theme::success(),
        "reject" | "rejected" => Theme::error(),
        "iterate" => Theme::warning(),
        _ => Theme::text_dim(),
    }
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
