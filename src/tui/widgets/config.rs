// src/tui/widgets/config.rs — Config viewer panel (Tab 6).

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::data::ConfigTree;
use crate::tui::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, data: &ConfigTree, scroll: u16) {
    let block = Block::default()
        .title(" Configuration ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let mut lines: Vec<Line> = Vec::new();

    for section in &data.sections {
        // Section header
        lines.push(Line::from(vec![Span::styled(
            format!("  [{}]", section.name),
            Theme::header(),
        )]));

        for (key, value) in &section.entries {
            lines.push(Line::from(vec![
                Span::styled(format!("    {:<28} ", key), Theme::text_dim()),
                Span::styled(value.as_str(), Theme::text()),
            ]));
        }

        lines.push(Line::from(""));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}
