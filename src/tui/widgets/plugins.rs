// src/tui/widgets/plugins.rs — Plugin and script status panel (Tab 5).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::data::PluginsData;
use crate::tui::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, data: &PluginsData) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    render_wasm(f, chunks[0], data);
    render_rhai(f, chunks[1], data);
    render_mcp(f, chunks[2], data);
}

fn render_wasm(f: &mut Frame, area: Rect, data: &PluginsData) {
    let block = Block::default()
        .title(format!(" WASM Plugins ({}) ", data.wasm_plugins.len()))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.wasm_plugins.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No WASM plugins configured.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = data
        .wasm_plugins
        .iter()
        .map(|name| {
            Line::from(vec![
                Span::styled("  \u{25cf} ", Theme::success()),
                Span::styled(name.as_str(), Theme::text()),
            ])
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_rhai(f: &mut Frame, area: Rect, data: &PluginsData) {
    let block = Block::default()
        .title(format!(" Rhai Scripts ({}) ", data.rhai_scripts.len()))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.rhai_scripts.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No Rhai scripts configured.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = data
        .rhai_scripts
        .iter()
        .map(|name| {
            Line::from(vec![
                Span::styled("  \u{25cf} ", Theme::success()),
                Span::styled(name.as_str(), Theme::text()),
            ])
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_mcp(f: &mut Frame, area: Rect, data: &PluginsData) {
    let block = Block::default()
        .title(format!(" MCP Servers ({}) ", data.mcp_servers.len()))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    if data.mcp_servers.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No MCP servers configured.",
            Theme::text_dim(),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = data
        .mcp_servers
        .iter()
        .map(|srv| {
            Line::from(vec![
                Span::styled("  \u{25cf} ", Theme::success()),
                Span::styled(srv.name.as_str(), Theme::text()),
                Span::styled(format!("  ({})", srv.command), Theme::text_dim()),
            ])
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}
