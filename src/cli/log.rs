use std::io::IsTerminal;
use std::time::Duration;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;

use crate::db::Db;
use crate::models::Event;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LogFilters {
    worker: Option<String>,
    ticket: Option<String>,
}

pub fn execute(
    follow: bool,
    limit: usize,
    worker: Option<String>,
    raw_filters: Vec<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;
    let use_color = std::io::stdout().is_terminal();
    let filters = parse_filters(worker, &raw_filters)?;

    let events =
        db.recent_events_filtered(filters.worker.as_deref(), filters.ticket.as_deref(), limit)?;
    print_events(&events, use_color);

    if follow {
        if use_color {
            loop {
                print!("\x1b[2J\x1b[H");
                let latest = db.recent_events_filtered(
                    filters.worker.as_deref(),
                    filters.ticket.as_deref(),
                    limit,
                )?;
                print_events(&latest, use_color);
                std::thread::sleep(Duration::from_secs(1));
            }
        } else {
            let mut last_id = events.first().map(|e| e.id).unwrap_or(0);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let new_events = db.recent_events_filtered(
                    filters.worker.as_deref(),
                    filters.ticket.as_deref(),
                    limit,
                )?;
                for event in new_events.iter().rev() {
                    if event.id > last_id {
                        println!("{}", format_event(event, use_color));
                        last_id = event.id;
                    }
                }
            }
        }
    }

    Ok(())
}

fn parse_filters(worker_arg: Option<String>, raw_filters: &[String]) -> Result<LogFilters> {
    let mut filters = LogFilters {
        worker: worker_arg,
        ticket: None,
    };
    for raw in raw_filters {
        let Some((key, value)) = raw.split_once('=') else {
            bail!("invalid --filter '{}', expected key=value", raw);
        };
        if value.trim().is_empty() {
            bail!("invalid --filter '{}', value cannot be empty", raw);
        }
        match key {
            "worker" => filters.worker = Some(value.to_string()),
            "ticket" => filters.ticket = Some(value.to_string()),
            _ => bail!(
                "unsupported --filter key '{}', supported: worker,ticket",
                key
            ),
        }
    }
    Ok(filters)
}

fn print_events(events: &[Event], use_color: bool) {
    for event in events.iter().rev() {
        println!("{}", format_event(event, use_color));
    }
}

fn format_event(event: &Event, use_color: bool) -> String {
    let worker = event.agent.as_deref().unwrap_or("-");
    let elapsed = format_elapsed(&event.timestamp);
    let tokens = event
        .tokens_used
        .map(|t| format!(" ({}tok)", t))
        .unwrap_or_default();
    let badge = format!("[{}]", worker);
    let event_label = event.event_type.clone();
    if !use_color {
        return format!(
            "[{}] {} {} {}: {}{}",
            event.timestamp, elapsed, badge, event_label, event.detail, tokens
        );
    }

    let styled_badge = color_by_event(&event.event_type, &badge).bold().to_string();
    let styled_event = color_by_event(&event.event_type, &event_label)
        .bold()
        .to_string();
    format!(
        "[{}] {} {} {}: {}{}",
        event.timestamp, elapsed, styled_badge, styled_event, event.detail, tokens
    )
}

fn format_elapsed(ts: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(ts) else {
        return "? ago".to_string();
    };
    let secs = Utc::now()
        .signed_duration_since(parsed.with_timezone(&Utc))
        .num_seconds();
    if secs < 0 {
        return "0s ago".to_string();
    }
    if secs < 60 {
        return format!("{}s ago", secs);
    }
    if secs < 3600 {
        return format!("{}m ago", secs / 60);
    }
    if secs < 86_400 {
        return format!("{}h ago", secs / 3600);
    }
    format!("{}d ago", secs / 86_400)
}

fn color_by_event<'a>(event_type: &str, text: &'a str) -> colored::ColoredString {
    match event_type {
        "completed" => text.green(),
        "error" => text.red(),
        "assigned" => text.yellow(),
        "merged" => text.blue(),
        _ => text.normal(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_filters() {
        let filters = parse_filters(
            Some("w-0".to_string()),
            &["ticket=t-001".to_string(), "worker=w-2".to_string()],
        )
        .unwrap();
        assert_eq!(
            filters,
            LogFilters {
                worker: Some("w-2".to_string()),
                ticket: Some("t-001".to_string())
            }
        );
    }

    #[test]
    fn rejects_unknown_filter_key() {
        let err = parse_filters(None, &["foo=bar".to_string()]).unwrap_err();
        assert!(err.to_string().contains("unsupported --filter key"));
    }

    #[test]
    fn rejects_filter_without_equals() {
        let err = parse_filters(None, &["worker".to_string()]).unwrap_err();
        assert!(err.to_string().contains("expected key=value"));
    }

    #[test]
    fn elapsed_handles_invalid_timestamp() {
        assert_eq!(format_elapsed("not-a-timestamp"), "? ago");
    }
}
