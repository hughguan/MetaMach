//! `herdr-janus` - MetaMach Herdr shadow client (Project-Plan M2 Tasks 2.2/2.3).
//!
//! Rendered inside the Herdr `overlay` pane. On wake it probes `janus.sock`;
//! if absent it lazy-starts `janus-daemon` detached (Feature-Spec §2.1), then
//! fetches live data. Two views toggled with `Tab`:
//!   - **Dispatch** - ACTIVE blueprints (selectable; dispatch lands in M4).
//!   - **Progress** - in-flight workflow tasks (Contract 3.3), polled at ~1s.
//!
//! `Esc`/`q` exits; `r` retries the Daemon. Focus stays locked inside the popup.

use std::io::{self, stdout};
use std::time::{Duration, Instant};

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
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use janus::paths;
use janus::protocol::{ActiveTask, BlueprintInfo, Request, Response};
use janus::{spawn, uds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Dispatch,
    Progress,
}

struct App {
    view: View,
    selected: usize,
    blueprints: Vec<BlueprintInfo>,
    tasks: Vec<ActiveTask>,
    daemon_online: bool,
    last_error: Option<String>,
    last_progress: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            view: View::Dispatch,
            selected: 0,
            blueprints: Vec::new(),
            tasks: Vec::new(),
            daemon_online: false,
            last_error: None,
            last_progress: Instant::now(),
        }
    }

    fn list_len(&self) -> usize {
        match self.view {
            View::Dispatch => self.blueprints.len(),
            View::Progress => self.tasks.len(),
        }
    }

    /// Move selection down, wrapping (UTC-01-03 wrap test).
    fn down(&mut self) {
        let n = self.list_len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected + 1) % n;
    }

    /// Move selection up, wrapping.
    fn up(&mut self) {
        let n = self.list_len();
        if n == 0 {
            return;
        }
        let last = n - 1;
        self.selected = if self.selected == 0 {
            last
        } else {
            self.selected - 1
        };
    }

    /// `Tab` toggles Dispatch <-> Progress, resetting selection and refreshing.
    fn toggle_view(&mut self) {
        self.flip_view();
        match self.view {
            View::Progress => self.poll_progress(),
            View::Dispatch => self.refresh_blueprints(),
        }
    }

    /// Flip Dispatch <-> Progress and reset selection (pure; no I/O) - split out
    /// so the flip+reset is unit-testable without a live `janus.sock` round-trip.
    fn flip_view(&mut self) {
        self.view = match self.view {
            View::Dispatch => View::Progress,
            View::Progress => View::Dispatch,
        };
        self.selected = 0;
    }

    /// Fetch ACTIVE blueprints from the Daemon (3s timeout).
    fn refresh_blueprints(&mut self) {
        match uds::request(&Request::Blueprints) {
            Ok(Response::Blueprints { blueprints }) => {
                self.blueprints = blueprints;
                self.daemon_online = true;
                self.last_error = None;
            }
            Ok(_) => self.daemon_online = true,
            Err(e) => {
                self.daemon_online = false;
                self.last_error = Some(e.to_string());
            }
        }
    }

    /// Poll the in-flight task snapshot (1s timeout; never blocks the TUI long).
    fn poll_progress(&mut self) {
        let req = Request::Progress { blueprint: None };
        match uds::request_to(&paths::sock_path(), &req, Duration::from_millis(1000)) {
            Ok(Response::Progress { active_tasks }) => {
                self.tasks = active_tasks;
                self.daemon_online = true;
                self.last_error = None;
            }
            Ok(_) => {}
            Err(e) => {
                self.daemon_online = false;
                self.last_error = Some(e.to_string());
            }
        }
        self.last_progress = Instant::now();
    }

    /// `r` - retry Daemon lazy-start + refresh.
    fn retry_daemon(&mut self) {
        if spawn::ensure_daemon(Duration::from_secs(5)).is_ok() {
            self.refresh_blueprints();
        } else {
            self.daemon_online = false;
            self.last_error = Some("daemon not reachable".to_string());
        }
    }
}

fn main() -> io::Result<()> {
    let mut app = App::new();
    if spawn::ensure_daemon(Duration::from_secs(5)).is_ok() {
        app.refresh_blueprints();
    } else {
        app.last_error = Some("daemon not reachable (press r to retry)".to_string());
    }
    let result = run(&mut app);
    restore_terminal();
    result
}

fn run(app: &mut App) -> io::Result<()> {
    setup_terminal()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| ui(f, app))?;
        if !event::poll(Duration::from_millis(200))? {
            // idle window: poll progress if the Progress view is due
            if app.view == View::Progress && app.last_progress.elapsed() >= Duration::from_secs(1) {
                app.poll_progress();
            }
            continue;
        }
        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Tab => app.toggle_view(),
                KeyCode::Down | KeyCode::Char('j') => app.down(),
                KeyCode::Up | KeyCode::Char('k') => app.up(),
                KeyCode::Char('r') => app.retry_daemon(),
                _ => {} // swallow: focus stays locked inside the popup
            }
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
    render_title(f, chunks[0], app);
    match app.view {
        View::Dispatch => render_dispatch(f, chunks[1], app),
        View::Progress => render_progress(f, chunks[1], app),
    }
    render_footer(f, chunks[2], app);
}

fn render_title(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let view = match app.view {
        View::Dispatch => "Dispatch",
        View::Progress => "Progress",
    };
    let title = Paragraph::new(format!("MetaMach 1.0 · {view}"))
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Dispatch (\u{2191}/\u{2193} select · Tab progress · r retry)");
    if app.blueprints.is_empty() {
        let msg = if app.daemon_online {
            "No ACTIVE blueprints. Onboard one: `janus onboard --blueprint <name>`"
        } else {
            "Daemon offline \u{2014} press r to retry / ensure `janus daemon` is running"
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::Yellow))
                .block(block),
            area,
        );
        return;
    }
    let rows = app.blueprints.iter().enumerate().map(|(i, b)| {
        let style = if i == app.selected {
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Row::new([
            Cell::from(b.name.as_str()),
            Cell::from(b.default_workflow.as_str()),
            Cell::from(b.remote_host.as_deref().unwrap_or("local")),
            Cell::from(b.status.as_str()),
        ])
        .style(style)
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
        Row::new(["Blueprint", "Workflow", "Host", "Status"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(block);
    f.render_widget(table, area);
}

fn render_progress(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Progress (Tab dispatch · 1s poll)");
    if app.tasks.is_empty() {
        let msg = if app.daemon_online {
            "No in-flight tasks."
        } else {
            "Daemon offline \u{2014} press r to retry"
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::Yellow))
                .block(block),
            area,
        );
        return;
    }
    let rows = app.tasks.iter().enumerate().map(|(i, t)| {
        let style = if i == app.selected {
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else if t.status == "SUSPENDED" {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        let step = t.current_step.as_deref().unwrap_or("-");
        let elapsed = t
            .elapsed_seconds
            .map(|s| format!("{s}s"))
            .unwrap_or_else(|| "?".to_string());
        Row::new([
            Cell::from(t.blueprint_id.as_str()),
            Cell::from(t.workflow_name.as_str()),
            Cell::from(step),
            Cell::from(t.status.as_str()),
            Cell::from(elapsed),
        ])
        .style(style)
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(
        Row::new(["Blueprint", "Workflow", "Step", "Status", "Elapsed"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(block);
    f.render_widget(table, area);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let status = if app.daemon_online {
        Span::styled("online", Style::default().fg(Color::Green))
    } else {
        Span::styled("offline", Style::default().fg(Color::Red))
    };
    let err = app
        .last_error
        .as_deref()
        .map(|e| format!(" \u{2014} {e}"))
        .unwrap_or_default();
    let line = Line::from(vec![
        Span::raw("Daemon "),
        status,
        Span::raw(err),
        Span::raw("   "),
        Span::styled(
            "Tab",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Green),
        ),
        Span::raw(" view   "),
        Span::styled(
            "Esc",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Green),
        ),
        Span::raw(" close   "),
        Span::styled(
            "r",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Green),
        ),
        Span::raw(" retry"),
    ]);
    f.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn setup_terminal() -> io::Result<()> {
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
    use janus::protocol::BlueprintInfo;

    fn sample_app() -> App {
        let mut app = App::new();
        app.blueprints = vec![
            BlueprintInfo {
                name: "joyrobots".into(),
                default_workflow: "dev-flow".into(),
                remote_host: None,
                status: "ACTIVE".into(),
            },
            BlueprintInfo {
                name: "gatemetric".into(),
                default_workflow: "firmware-deploy".into(),
                remote_host: Some("192.168.1.100".into()),
                status: "ACTIVE".into(),
            },
        ];
        app
    }

    #[test]
    fn selection_wraps_in_dispatch() {
        let mut app = sample_app();
        assert_eq!(app.list_len(), 2);
        app.down();
        app.down();
        assert_eq!(app.selected, 0, "should wrap to top");
        app.up();
        assert_eq!(app.selected, 1, "should wrap to bottom");
    }

    #[test]
    fn flip_view_flips_and_resets_selection() {
        let mut app = sample_app();
        app.down();
        assert_eq!(app.selected, 1);
        assert_eq!(app.view, View::Dispatch);

        app.flip_view();
        assert_eq!(app.view, View::Progress);
        assert_eq!(app.selected, 0, "selection resets on flip to Progress");

        app.selected = 1; // simulate navigation in the new view
        app.flip_view();
        assert_eq!(app.view, View::Dispatch);
        assert_eq!(app.selected, 0, "selection resets on flip back to Dispatch");
    }

    #[test]
    fn ui_renders_dispatch_view() {
        use ratatui::backend::TestBackend;

        let app = sample_app();
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();

        let buf = terminal.backend().buffer();
        let text: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(text.contains("Dispatch"));
        assert!(text.contains("joyrobots"));
        assert!(text.contains("gatemetric"));
        assert!(text.contains("Daemon"));
    }
}
