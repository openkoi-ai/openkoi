// src/tui/theme.rs — Color scheme and style definitions for the TUI dashboard.

use ratatui::style::{Color, Modifier, Style};

/// Koi-inspired color palette.
pub struct Theme;

impl Theme {
    // ── Brand colors ─────────────────────────────────────────────
    pub const KOI_ORANGE: Color = Color::Rgb(255, 140, 50);
    pub const KOI_WHITE: Color = Color::Rgb(240, 240, 240);
    pub const KOI_DARK: Color = Color::Rgb(20, 20, 30);
    pub const KOI_BLUE: Color = Color::Rgb(70, 130, 220);
    pub const KOI_GREEN: Color = Color::Rgb(80, 200, 120);
    pub const KOI_RED: Color = Color::Rgb(230, 80, 80);
    pub const KOI_YELLOW: Color = Color::Rgb(230, 200, 60);
    pub const KOI_GRAY: Color = Color::Rgb(120, 120, 140);
    pub const KOI_DIM: Color = Color::Rgb(80, 80, 100);
    pub const KOI_CYAN: Color = Color::Rgb(80, 200, 220);

    // ── Semantic styles ──────────────────────────────────────────

    /// Active/selected tab header.
    pub fn tab_active() -> Style {
        Style::default()
            .fg(Theme::KOI_ORANGE)
            .add_modifier(Modifier::BOLD)
    }

    /// Inactive tab header.
    pub fn tab_inactive() -> Style {
        Style::default().fg(Theme::KOI_GRAY)
    }

    /// Main title / header bar.
    pub fn header() -> Style {
        Style::default()
            .fg(Theme::KOI_ORANGE)
            .add_modifier(Modifier::BOLD)
    }

    /// Block border (normal).
    pub fn border() -> Style {
        Style::default().fg(Theme::KOI_DIM)
    }

    /// Block border (focused / selected).
    pub fn border_focus() -> Style {
        Style::default().fg(Theme::KOI_ORANGE)
    }

    /// Normal body text.
    pub fn text() -> Style {
        Style::default().fg(Theme::KOI_WHITE)
    }

    /// Dimmed / secondary text.
    pub fn text_dim() -> Style {
        Style::default().fg(Theme::KOI_GRAY)
    }

    /// Success indicator.
    pub fn success() -> Style {
        Style::default().fg(Theme::KOI_GREEN)
    }

    /// Warning indicator.
    pub fn warning() -> Style {
        Style::default().fg(Theme::KOI_YELLOW)
    }

    /// Error / critical indicator.
    pub fn error() -> Style {
        Style::default().fg(Theme::KOI_RED)
    }

    /// Informational / accent.
    pub fn info() -> Style {
        Style::default().fg(Theme::KOI_BLUE)
    }

    /// Highlight / emphasis.
    pub fn highlight() -> Style {
        Style::default()
            .fg(Theme::KOI_CYAN)
            .add_modifier(Modifier::BOLD)
    }

    /// Table header row.
    pub fn table_header() -> Style {
        Style::default()
            .fg(Theme::KOI_ORANGE)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    /// Selected table row.
    pub fn table_selected() -> Style {
        Style::default()
            .bg(Color::Rgb(40, 40, 60))
            .fg(Theme::KOI_WHITE)
    }

    /// Key hint in the footer.
    pub fn key_hint() -> Style {
        Style::default().fg(Theme::KOI_ORANGE)
    }

    /// Description next to key hint.
    pub fn key_desc() -> Style {
        Style::default().fg(Theme::KOI_GRAY)
    }

    /// Style for a confidence value (color-coded).
    pub fn confidence(value: f64) -> Style {
        if value >= 0.8 {
            Style::default().fg(Theme::KOI_GREEN)
        } else if value >= 0.5 {
            Style::default().fg(Theme::KOI_YELLOW)
        } else {
            Style::default().fg(Theme::KOI_RED)
        }
    }

    /// Style for a score value (color-coded 0.0–1.0).
    pub fn score(value: f64) -> Style {
        Self::confidence(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_high() {
        let s = Theme::confidence(0.9);
        assert_eq!(s.fg, Some(Theme::KOI_GREEN));
    }

    #[test]
    fn test_confidence_medium() {
        let s = Theme::confidence(0.6);
        assert_eq!(s.fg, Some(Theme::KOI_YELLOW));
    }

    #[test]
    fn test_confidence_low() {
        let s = Theme::confidence(0.3);
        assert_eq!(s.fg, Some(Theme::KOI_RED));
    }

    #[test]
    fn test_confidence_boundary_08() {
        // Exactly 0.8 should be green
        let s = Theme::confidence(0.8);
        assert_eq!(s.fg, Some(Theme::KOI_GREEN));
    }

    #[test]
    fn test_confidence_boundary_05() {
        // Exactly 0.5 should be yellow
        let s = Theme::confidence(0.5);
        assert_eq!(s.fg, Some(Theme::KOI_YELLOW));
    }

    #[test]
    fn test_score_delegates_to_confidence() {
        // score() should produce same result as confidence()
        let score_style = Theme::score(0.85);
        let conf_style = Theme::confidence(0.85);
        assert_eq!(score_style, conf_style);
    }

    #[test]
    fn test_tab_active_is_orange_bold() {
        let s = Theme::tab_active();
        assert_eq!(s.fg, Some(Theme::KOI_ORANGE));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_tab_inactive_is_gray() {
        let s = Theme::tab_inactive();
        assert_eq!(s.fg, Some(Theme::KOI_GRAY));
    }

    #[test]
    fn test_header_style() {
        let s = Theme::header();
        assert_eq!(s.fg, Some(Theme::KOI_ORANGE));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_error_style() {
        let s = Theme::error();
        assert_eq!(s.fg, Some(Theme::KOI_RED));
    }

    #[test]
    fn test_success_style() {
        let s = Theme::success();
        assert_eq!(s.fg, Some(Theme::KOI_GREEN));
    }

    #[test]
    fn test_table_header_style() {
        let s = Theme::table_header();
        assert_eq!(s.fg, Some(Theme::KOI_ORANGE));
        assert!(s.add_modifier.contains(Modifier::BOLD));
        assert!(s.add_modifier.contains(Modifier::UNDERLINED));
    }
}
