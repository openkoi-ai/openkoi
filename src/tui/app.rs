// src/tui/app.rs — TUI application state, event loop, and rendering.

use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, TableState, Tabs},
    Frame, Terminal,
};

use crate::infra::config::Config;
use crate::memory::store::Store;

use super::data::{self, DashboardData};
use super::theme::Theme;
use super::widgets;

// ── Tab enum ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Tasks,
    Learnings,
    Costs,
    Plugins,
    Config,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Tasks,
        Tab::Learnings,
        Tab::Costs,
        Tab::Plugins,
        Tab::Config,
    ];

    fn label(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Tasks => "Tasks",
            Tab::Learnings => "Learnings",
            Tab::Costs => "Costs",
            Tab::Plugins => "Plugins",
            Tab::Config => "Config",
        }
    }

    fn index(&self) -> usize {
        Tab::ALL.iter().position(|t| t == self).unwrap_or(0)
    }

    fn from_index(i: usize) -> Tab {
        *Tab::ALL.get(i).unwrap_or(&Tab::Overview)
    }
}

// ── App state ────────────────────────────────────────────────────

struct App {
    active_tab: Tab,
    data: DashboardData,
    last_refresh: Instant,

    // Per-tab interactive state
    task_table_state: TableState,
    learning_table_state: TableState,
    config_scroll: u16,
}

impl App {
    fn new(data: DashboardData) -> Self {
        Self {
            active_tab: Tab::Overview,
            data,
            last_refresh: Instant::now(),
            task_table_state: TableState::default(),
            learning_table_state: TableState::default(),
            config_scroll: 0,
        }
    }

    fn next_tab(&mut self) {
        let idx = self.active_tab.index();
        self.active_tab = Tab::from_index((idx + 1) % Tab::ALL.len());
    }

    fn prev_tab(&mut self) {
        let idx = self.active_tab.index();
        self.active_tab = Tab::from_index((idx + Tab::ALL.len() - 1) % Tab::ALL.len());
    }

    fn scroll_down(&mut self) {
        match self.active_tab {
            Tab::Tasks => {
                let i = self.task_table_state.selected().unwrap_or(0);
                let max = self.data.tasks.tasks.len().saturating_sub(1);
                self.task_table_state.select(Some((i + 1).min(max)));
            }
            Tab::Learnings => {
                let i = self.learning_table_state.selected().unwrap_or(0);
                let max = self.data.learnings.learnings.len().saturating_sub(1);
                self.learning_table_state.select(Some((i + 1).min(max)));
            }
            Tab::Config => {
                self.config_scroll = self.config_scroll.saturating_add(1);
            }
            _ => {}
        }
    }

    fn scroll_up(&mut self) {
        match self.active_tab {
            Tab::Tasks => {
                let i = self.task_table_state.selected().unwrap_or(0);
                self.task_table_state.select(Some(i.saturating_sub(1)));
            }
            Tab::Learnings => {
                let i = self.learning_table_state.selected().unwrap_or(0);
                self.learning_table_state.select(Some(i.saturating_sub(1)));
            }
            Tab::Config => {
                self.config_scroll = self.config_scroll.saturating_sub(1);
            }
            _ => {}
        }
    }
}

// ── Public entry point ───────────────────────────────────────────

/// Launch the TUI dashboard. Blocks until the user quits (q / Esc / Ctrl-C).
pub fn run_dashboard(store: Option<&Store>, config: &Config) -> anyhow::Result<()> {
    // Initial data load
    let data = data::fetch_all(store, config);
    let mut app = App::new(data);

    // Select first row in tables if data exists
    if !app.data.tasks.tasks.is_empty() {
        app.task_table_state.select(Some(0));
    }
    if !app.data.learnings.learnings.is_empty() {
        app.learning_table_state.select(Some(0));
    }

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_event_loop(&mut terminal, &mut app, store, config);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: Option<&Store>,
    config: &Config,
) -> anyhow::Result<()> {
    let refresh_interval = Duration::from_secs(5);

    loop {
        // Draw
        terminal.draw(|f| render(f, app))?;

        // Auto-refresh data periodically
        if app.last_refresh.elapsed() >= refresh_interval {
            app.data = data::fetch_all(store, config);
            app.last_refresh = Instant::now();
        }

        // Poll for events (250ms timeout for responsive refresh)
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                // Quit
                if key.code == KeyCode::Char('q')
                    || key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    return Ok(());
                }

                match key.code {
                    // Tab navigation
                    KeyCode::Tab | KeyCode::Right => app.next_tab(),
                    KeyCode::BackTab | KeyCode::Left => app.prev_tab(),

                    // Number keys for direct tab access
                    KeyCode::Char('1') => app.active_tab = Tab::Overview,
                    KeyCode::Char('2') => app.active_tab = Tab::Tasks,
                    KeyCode::Char('3') => app.active_tab = Tab::Learnings,
                    KeyCode::Char('4') => app.active_tab = Tab::Costs,
                    KeyCode::Char('5') => app.active_tab = Tab::Plugins,
                    KeyCode::Char('6') => app.active_tab = Tab::Config,

                    // Scrolling
                    KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),

                    // Manual refresh
                    KeyCode::Char('r') => {
                        app.data = data::fetch_all(store, config);
                        app.last_refresh = Instant::now();
                    }

                    _ => {}
                }
            }
        }
    }
}

// ── Rendering ────────────────────────────────────────────────────

fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header + tabs
            Constraint::Min(10),   // Main content
            Constraint::Length(1), // Footer / key hints
        ])
        .split(size);

    render_header(f, chunks[0], app);
    render_tab_content(f, chunks[1], app);
    render_footer(f, chunks[2], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let label = format!(" {} {} ", i + 1, tab.label());
            if *tab == app.active_tab {
                Line::from(Span::styled(label, Theme::tab_active()))
            } else {
                Line::from(Span::styled(label, Theme::tab_inactive()))
            }
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .title(Span::styled(" OpenKoi Dashboard ", Theme::header()))
                .borders(Borders::ALL)
                .border_style(Theme::border()),
        )
        .select(app.active_tab.index())
        .highlight_style(Theme::tab_active())
        .divider(Span::styled(" | ", Theme::text_dim()));

    f.render_widget(tabs, area);
}

fn render_tab_content(f: &mut Frame, area: Rect, app: &mut App) {
    match app.active_tab {
        Tab::Overview => widgets::overview::render(f, area, &app.data.overview),
        Tab::Tasks => widgets::tasks::render(f, area, &app.data.tasks, &mut app.task_table_state),
        Tab::Learnings => {
            widgets::learnings::render(f, area, &app.data.learnings, &mut app.learning_table_state)
        }
        Tab::Costs => widgets::costs::render(f, area, &app.data.costs),
        Tab::Plugins => widgets::plugins::render(f, area, &app.data.plugins),
        Tab::Config => widgets::config::render(f, area, &app.data.config_tree, app.config_scroll),
    }
}

fn render_footer(f: &mut Frame, area: Rect, _app: &App) {
    let hints = Line::from(vec![
        Span::styled(" q", Theme::key_hint()),
        Span::styled(" quit  ", Theme::key_desc()),
        Span::styled("Tab/\u{2190}\u{2192}", Theme::key_hint()),
        Span::styled(" switch  ", Theme::key_desc()),
        Span::styled("1-6", Theme::key_hint()),
        Span::styled(" jump  ", Theme::key_desc()),
        Span::styled("j/k/\u{2191}\u{2193}", Theme::key_hint()),
        Span::styled(" scroll  ", Theme::key_desc()),
        Span::styled("r", Theme::key_hint()),
        Span::styled(" refresh", Theme::key_desc()),
    ]);

    let p = Paragraph::new(hints);
    f.render_widget(p, area);
}
