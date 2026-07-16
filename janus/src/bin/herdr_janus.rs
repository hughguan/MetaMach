//! `herdr-janus` - MetaMach Herdr shadow client (Project-Plan M1, Task 1.2).
//!
//! Renders a static "production dispatch dashboard" inside the Herdr `overlay`
//! pane declared by `herdr-plugin.toml`. M1 is deliberately stateless: there is
//! no `janus-daemon` UDS connection yet (that lands in M2 Task 2.2), so the
//! dashboard shows placeholder dispatch rows and reports the Daemon as offline.
//!
//! Behavioral contract (docs/herdr-v1-contract.md §8, docs/Feature-Spec.md §2.1,
//! Test-Spec UTC-01-03):
//!   - opened by Herdr as an `overlay` pane running this binary;
//!   - input focus is auto-locked (raw mode + own event loop swallows every key
//!     so nothing reaches the background pane);
//!   - `Esc` restores the terminal and exits (the M1 "pop the stack").
//!
//! Runs standalone (`cargo run --bin herdr-janus`) for a smoke test without Herdr.

use std::io::{self, stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

/// A blueprint the Factory Director can dispatch from the Popup.
///
/// M1 serves static placeholder rows; M2 replaces this with a live
/// `SELECT ... FROM blueprints WHERE status = 'ACTIVE'` snapshot fetched from
/// `janus-daemon` over `janus.sock`.
#[derive(Clone)]
struct DispatchEntry {
    name: &'static str,
    workflow: &'static str,
    host: &'static str,
    status: &'static str,
}

fn dispatch_entries() -> Vec<DispatchEntry> {
    vec![
        DispatchEntry {
            name: "joyrobots",
            workflow: "dev-flow",
            host: "local",
            status: "ACTIVE",
        },
        DispatchEntry {
            name: "gatemetric",
            workflow: "firmware-deploy",
            host: "remote",
            status: "ACTIVE",
        },
    ]
}

/// Daemon connection state shown in the footer. M1 is always offline (no UDS
/// yet); M2 flips this to `Online` once `janus.sock` answers.
fn daemon_state() -> &'static str {
    "Offline (live data in M2)"
}

struct App {
    entries: Vec<DispatchEntry>,
    selected: usize,
    daemon: &'static str,
}

impl Default for App {
    fn default() -> Self {
        Self {
            entries: dispatch_entries(),
            selected: 0,
            daemon: daemon_state(),
        }
    }
}

impl App {
    /// Move selection down, wrapping to the top at the end (UTC-01-03 wrap test).
    fn down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.entries.len();
    }

    /// Move selection up, wrapping to the bottom at the top.
    fn up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        self.selected = if self.selected == 0 {
            last
        } else {
            self.selected - 1
        };
    }
}

fn main() -> io::Result<()> {
    let mut app = App::default();
    let result = run(&mut app);
    // Always restore the terminal, even on error - never leave raw mode on.
    restore_terminal();
    result
}

fn run(app: &mut App) -> io::Result<()> {
    setup_terminal()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| ui(f, app))?;
        // Poll so we don't spin; 250ms is well under any noticeable render lag.
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Down | KeyCode::Char('j') => app.down(),
                KeyCode::Up | KeyCode::Char('k') => app.up(),
                // Swallow every other key: focus stays locked inside the popup
                // (UTC-01-03) - no keystroke reaches the background tiled pane.
                _ => {}
            },
            // Swallow mouse/resize/other events; focus stays locked.
            _ => {}
        }
    }
    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .split(area);
    render_title(f, chunks[0]);
    render_dispatch(f, chunks[1], app);
    render_footer(f, chunks[2], app);
}

fn render_title(f: &mut ratatui::Frame, area: Rect) {
    let title = Paragraph::new("MetaMach 1.0 - Production Dispatch Dashboard")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, area);
}

fn render_dispatch(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let header = ["Blueprint", "Workflow", "Host", "Status"];
    let rows = app.entries.iter().enumerate().map(|(i, e)| {
        let selected = i == app.selected;
        let style = if selected {
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Row::new([e.name, e.workflow, e.host, e.status]).style(style)
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
        ],
    )
    .header(
        Row::new(header).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Dispatch (\u{2191}/\u{2193} select · Enter dispatches in M2)"),
    );
    f.render_widget(table, area);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled("Daemon: ", Style::default().fg(Color::Gray)),
        Span::styled(app.daemon, Style::default().fg(Color::Red)),
        Span::raw("   "),
        Span::styled(
            "Esc",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Green),
        ),
        Span::raw(" close   "),
        Span::styled(
            "\u{2191}/\u{2193}",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Green),
        ),
        Span::raw(" navigate"),
    ]);
    let footer = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
}

fn setup_terminal() -> io::Result<()> {
    // Install the panic hook first so the terminal is restored even if a later
    // step panics before the main loop's own restore runs.
    install_panic_hook();
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Ok(())
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_entries_are_all_active() {
        let entries = dispatch_entries();
        assert!(
            !entries.is_empty(),
            "dispatch table must show at least one blueprint"
        );
        assert!(
            entries.iter().all(|e| e.status == "ACTIVE"),
            "M1 placeholder rows should all be ACTIVE (Onboard sets this in M4)"
        );
    }

    #[test]
    fn selection_wraps_top_and_bottom() {
        let mut app = App::default();
        let n = app.entries.len();
        assert!(n >= 2, "test assumes at least two entries");

        // Pressing down n times returns to the original selection (wrap).
        for _ in 0..n {
            app.down();
        }
        assert_eq!(app.selected, 0);

        // One more wraps to index 1.
        app.down();
        assert_eq!(app.selected, 1);

        // Up from 0 wraps to the last index.
        app.selected = 0;
        app.up();
        assert_eq!(app.selected, n - 1);
    }

    #[test]
    fn ui_renders_without_panic() {
        use ratatui::backend::TestBackend;

        let app = App::default();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();

        let buf = terminal.backend().buffer();
        let text: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(
            text.contains("Dispatch"),
            "dispatch table title should render"
        );
        assert!(
            text.contains("joyrobots"),
            "placeholder blueprint row should render"
        );
        assert!(
            text.contains("Daemon"),
            "daemon status footer should render"
        );
    }
}
