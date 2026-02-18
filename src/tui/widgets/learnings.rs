// src/tui/widgets/learnings.rs — Learnings and patterns browser (Tab 3).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::tui::data::LearningsData;
use crate::tui::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, data: &LearningsData, table_state: &mut TableState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Percentage(30),
            Constraint::Percentage(25),
        ])
        .split(area);

    render_learnings_table(f, chunks[0], data, table_state);
    render_patterns(f, chunks[1], data);
    render_skills(f, chunks[2], data);
}

fn render_learnings_table(f: &mut Frame, area: Rect, data: &LearningsData, state: &mut TableState) {
    let header = Row::new(vec![
        Cell::from("Type").style(Theme::table_header()),
        Cell::from("Content").style(Theme::table_header()),
        Cell::from("Conf").style(Theme::table_header()),
        Cell::from("Reinf").style(Theme::table_header()),
        Cell::from("Category").style(Theme::table_header()),
    ]);

    let rows: Vec<Row> = data
        .learnings
        .iter()
        .map(|l| {
            let conf_style = Theme::confidence(l.confidence);
            Row::new(vec![
                Cell::from(l.learning_type.clone()).style(type_style(&l.learning_type)),
                Cell::from(truncate(&l.content, 60)).style(Theme::text()),
                Cell::from(format!("{:.2}", l.confidence)).style(conf_style),
                Cell::from(l.reinforced.to_string()).style(Theme::text_dim()),
                Cell::from(l.category.as_deref().unwrap_or("-").to_string())
                    .style(Theme::text_dim()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Min(30),
        Constraint::Length(6),
        Constraint::Length(5),
        Constraint::Length(14),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(format!(" Learnings ({}) ", data.learnings.len()))
                .borders(Borders::ALL)
                .border_style(Theme::border()),
        )
        .row_highlight_style(Theme::table_selected())
        .highlight_symbol(">> ");

    f.render_stateful_widget(table, area, state);
}

fn render_patterns(f: &mut Frame, area: Rect, data: &LearningsData) {
    let block = Block::default()
        .title(format!(" Detected Patterns ({}) ", data.patterns.len()))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.patterns.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No patterns detected yet.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = data
        .patterns
        .iter()
        .take(area.height.saturating_sub(2) as usize)
        .map(|p| {
            let status_style = match p.status.as_deref().unwrap_or("detected") {
                "detected" => Theme::info(),
                "proposed" => Theme::warning(),
                "accepted" => Theme::success(),
                "rejected" => Theme::error(),
                _ => Theme::text_dim(),
            };
            Line::from(vec![
                Span::styled(
                    format!(" {:>10} ", p.status.as_deref().unwrap_or("detected")),
                    status_style,
                ),
                Span::styled(format!("[{}] ", p.pattern_type), Theme::text_dim()),
                Span::styled(truncate(&p.description, 50), Theme::text()),
                Span::styled(
                    format!("  conf={:.2} samples={}", p.confidence, p.sample_count),
                    Theme::text_dim(),
                ),
            ])
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_skills(f: &mut Frame, area: Rect, data: &LearningsData) {
    let block = Block::default()
        .title(format!(" Skill Effectiveness ({}) ", data.skills.len()))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.skills.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No skill effectiveness data yet.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = data
        .skills
        .iter()
        .take(area.height.saturating_sub(2) as usize)
        .map(|s| {
            let score_style = Theme::score(s.avg_score);
            Line::from(vec![
                Span::styled(format!("  {:<20} ", s.skill_name), Theme::text()),
                Span::styled(format!("[{}] ", s.task_category), Theme::text_dim()),
                Span::styled(format!("avg={:.2}", s.avg_score), score_style),
                Span::styled(format!("  ({} samples)", s.sample_count), Theme::text_dim()),
            ])
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn type_style(learning_type: &str) -> ratatui::style::Style {
    match learning_type {
        "preference" => Theme::info(),
        "pattern" => Theme::highlight(),
        "skill" => Theme::success(),
        "correction" => Theme::warning(),
        "anti-pattern" => Theme::error(),
        _ => Theme::text(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
