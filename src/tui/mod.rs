// src/tui/mod.rs — TUI dashboard module.
//
// Provides a terminal-based dashboard for OpenKoi, built with ratatui.
// Launch via `openkoi dashboard`.

pub mod app;
pub mod data;
pub mod theme;
pub mod widgets;

pub use app::run_dashboard;
