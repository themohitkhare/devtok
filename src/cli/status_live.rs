use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Row, Table},
    Terminal,
};
use std::{
    io,
    time::{Duration, Instant},
};

use crate::cli::cost::fmt_tokens;
use crate::db::Db;
use crate::models::pricing;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const LOG_TAIL_LINES: usize = 10;

struct AppState {
    /// (status, count) pairs
    ticket_counts: Vec<(String, i64)>,
    /// agents list
    agents: Vec<crate::models::Agent>,
    /// recent events (newest first)
    recent_events: Vec<crate::models::Event>,
    /// total input and output tokens
    input_tokens: i64,
    output_tokens: i64,
    /// Last refresh timestamp for display
    last_refresh: chrono::DateTime<chrono::Local>,
}

impl AppState {
    fn load(db: &Db) -> Result<Self> {
        let ticket_counts = db.count_by_status()?;
        let agents = db.list_agents()?;
        let recent_events = db.recent_events(LOG_TAIL_LINES)?;
        let (input_tokens, output_tokens) = db.total_token_details()?;
        Ok(AppState {
            ticket_counts,
            agents,
            recent_events,
            input_tokens,
            output_tokens,
            last_refresh: chrono::Local::now(),
        })
    }

    fn completed(&self) -> i64 {
        self.ticket_counts
            .iter()
            .find(|(s, _)| s == "completed")
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    fn total(&self) -> i64 {
        self.ticket_counts.iter().map(|(_, c)| c).sum()
    }
}

pub fn run(db: &Db) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, db);

    // Restore terminal regardless of outcome
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, db: &Db) -> Result<()> {
    let mut last_tick = Instant::now() - REFRESH_INTERVAL; // force immediate first load
    let mut state = AppState::load(db)?;

    loop {
        // Refresh data if interval elapsed
        if last_tick.elapsed() >= REFRESH_INTERVAL {
            state = AppState::load(db)?;
            last_tick = Instant::now();
        }

        terminal.draw(|f| draw(f, &state))?;

        // Poll for input (with a short timeout so we keep refreshing)
        let timeout = REFRESH_INTERVAL
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match (key.modifiers, key.code) {
                    // Ctrl+C or 'q' to exit
                    (KeyModifiers::CONTROL, KeyCode::Char('c'))
                    | (_, KeyCode::Char('q')) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

fn draw(f: &mut ratatui::Frame, state: &AppState) {
    let area = f.area();

    // Outer layout: title bar + body + footer
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title
            Constraint::Min(10),   // body
            Constraint::Length(1), // footer hint
        ])
        .split(area);

    draw_title(f, outer[0], state);
    draw_body(f, outer[1], state);
    draw_footer(f, outer[2]);
}

fn draw_title(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let refresh_str = state.last_refresh.format("%H:%M:%S").to_string();
    let title = Paragraph::new(format!(
        " ACS Live Status Monitor  —  Last refresh: {}",
        refresh_str
    ))
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, area);
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect) {
    let hint = Paragraph::new(" Ctrl+C / q: exit  •  auto-refresh every 2s")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, area);
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    // Body: top row (workers + progress) + log + tokens
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // workers + progress bar
            Constraint::Min(6),     // log tail
            Constraint::Length(3),  // token counter
        ])
        .split(area);

    // Top row: workers | ticket progress
    let top_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body[0]);

    draw_workers(f, top_row[0], state);
    draw_progress(f, top_row[1], state);
    draw_log(f, body[1], state);
    draw_tokens(f, body[2], state);
}

fn draw_workers(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let header_cells = ["Agent", "Role", "Status", "Ticket"]
        .iter()
        .map(|h| ratatui::widgets::Cell::from(*h).style(Style::default().add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells)
        .style(Style::default().fg(Color::Yellow))
        .height(1);

    let rows: Vec<Row> = state.agents.iter().map(|a| {
        let status_color = match a.status.as_str() {
            "working" => Color::Green,
            "idle" => Color::Gray,
            _ => Color::White,
        };
        Row::new(vec![
            ratatui::widgets::Cell::from(a.id.clone()),
            ratatui::widgets::Cell::from(a.role.clone()),
            ratatui::widgets::Cell::from(a.status.clone())
                .style(Style::default().fg(status_color)),
            ratatui::widgets::Cell::from(
                a.current_ticket.clone().unwrap_or_else(|| "-".to_string()),
            ),
        ])
    }).collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(" Workers "));

    f.render_widget(table, area);
}

fn draw_progress(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // gauge
            Constraint::Min(1),    // status breakdown
        ])
        .split(
            Block::default()
                .borders(Borders::ALL)
                .title(" Ticket Progress ")
                .inner(area),
        );

    // Gauge
    let completed = state.completed();
    let total = state.total();
    let ratio = if total > 0 {
        (completed as f64 / total as f64).min(1.0)
    } else {
        0.0
    };
    let gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .ratio(ratio)
        .label(format!("{}/{} completed", completed, total));
    f.render_widget(gauge, inner[0]);

    // Status breakdown
    let mut items: Vec<ListItem> = state
        .ticket_counts
        .iter()
        .map(|(status, count)| {
            let color = match status.as_str() {
                "completed" => Color::Green,
                "in_progress" => Color::Yellow,
                "pending" => Color::Gray,
                "blocked" => Color::Red,
                "review_pending" => Color::Cyan,
                _ => Color::White,
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<16}", status), Style::default().fg(color)),
                Span::raw(format!("{}", count)),
            ]))
        })
        .collect();

    if items.is_empty() {
        items.push(ListItem::new("No tickets"));
    }

    let list = List::new(items);
    f.render_widget(list, inner[1]);

    // Render the outer block separately
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(" Ticket Progress ");
    f.render_widget(outer_block, area);
}

fn draw_log(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let items: Vec<ListItem> = state
        .recent_events
        .iter()
        .rev() // oldest first for natural log reading
        .map(|e| {
            let agent = e.agent.as_deref().unwrap_or("-");
            let tokens = e
                .tokens_used
                .filter(|&t| t > 0)
                .map(|t| format!(" [{}tok]", t))
                .unwrap_or_default();
            // Truncate timestamp to HH:MM:SS for brevity
            let ts = if e.timestamp.len() >= 19 {
                &e.timestamp[11..19]
            } else {
                e.timestamp.as_str()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", ts), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", agent), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{}: ", e.event_type), Style::default().fg(Color::Cyan)),
                Span::raw(format!("{}{}", e.detail, tokens)),
            ]))
        })
        .collect();

    let log_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Recent Events "));
    f.render_widget(log_list, area);
}

fn draw_tokens(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let total = state.input_tokens + state.output_tokens;
    let sonnet_cost = pricing::estimate_cost(
        state.input_tokens,
        state.output_tokens,
        pricing::SONNET_INPUT_PER_M,
        pricing::SONNET_OUTPUT_PER_M,
    );
    let text = format!(
        " Total: {}  ({} in / {} out)   Est. cost: ${:.4} Sonnet",
        fmt_tokens(total),
        fmt_tokens(state.input_tokens),
        fmt_tokens(state.output_tokens),
        sonnet_cost,
    );
    let tokens_widget = Paragraph::new(text)
        .style(Style::default().fg(Color::Magenta))
        .block(Block::default().borders(Borders::ALL).title(" Token Usage "));
    f.render_widget(tokens_widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_counts_only_completed_status() {
        let state = AppState {
            ticket_counts: vec![
                ("completed".to_string(), 3),
                ("pending".to_string(), 2),
                ("blocked".to_string(), 1),
            ],
            agents: vec![],
            recent_events: vec![],
            input_tokens: 0,
            output_tokens: 0,
            last_refresh: chrono::Local::now(),
        };

        assert_eq!(state.completed(), 3);
    }

    #[test]
    fn total_is_sum_of_all_ticket_counts() {
        let state = AppState {
            ticket_counts: vec![
                ("completed".to_string(), 3),
                ("pending".to_string(), 2),
                ("blocked".to_string(), 1),
            ],
            agents: vec![],
            recent_events: vec![],
            input_tokens: 0,
            output_tokens: 0,
            last_refresh: chrono::Local::now(),
        };

        assert_eq!(state.total(), 6);
    }

    #[test]
    fn completed_zero_when_no_completed_status() {
        let state = AppState {
            ticket_counts: vec![("pending".to_string(), 2)],
            agents: vec![],
            recent_events: vec![],
            input_tokens: 0,
            output_tokens: 0,
            last_refresh: chrono::Local::now(),
        };

        assert_eq!(state.completed(), 0);
    }
}
