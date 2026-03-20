# Design: Agent Logs Panel in `acs status --live`

**Date:** 2026-03-20
**Status:** Approved

## Problem

`acs status --live` shows a "Recent Events" panel sourced from the SQLite DB. These are high-level manager events (ticket assigned, completed, timed out). There is no way to see what a worker agent is actually doing right now — the raw output from its Claude/Codex/Cursor subprocess — without leaving the TUI and reading `.acs/logs/<worker_id>.log` manually.

## Goal

Add an **Agent Logs** panel to the live dashboard that shows a live tail of all active workers' log files, interleaved in a single scrollable view, refreshing every 2 seconds.

## Layout

The existing "Recent Events" row is split horizontally 50/50:

```
┌─────────────────────────────────────────────────────────────────┐
│  ACS Live Status Monitor  —  Last refresh: HH:MM:SS             │
├──────────────────────────────┬──────────────────────────────────┤
│  Workers                     │  Ticket Progress                 │
│  (agent table)               │  [======== 42/48 ======]        │
│                              │  completed   42                  │
│                              │  pending      6                  │
├──────────────────────────────┬──────────────────────────────────┤
│  Recent Events               │  Agent Logs                      │
│  [DB event log]              │  [w-0] Reading KB entries...     │
│                              │  [w-1] cargo test passed         │
│                              │  [w-2] Committing changes        │
│                              │  [w-0] Notifying manager         │
├──────────────────────────────┴──────────────────────────────────┤
│  Token Usage                                                     │
├─────────────────────────────────────────────────────────────────┤
│  Ctrl+C / q: exit  •  auto-refresh every 2s                     │
└─────────────────────────────────────────────────────────────────┘
```

## Data Loading

### Log file location

`.acs/logs/<worker_id>.log` — one file per worker, written by `Spawner::spawn_provider`.

### Loading logic (in `AppState::load`)

1. Filter `agents` to those with `status == "working"`.
2. For each active worker, read the tail of its log file:
   - Use `std::fs::read_to_string` and split on newlines.
   - Take the last `tail_lines_per_worker` lines where `tail_lines_per_worker = max(1, PANEL_HEIGHT / active_worker_count)`.
   - Skip silently if the file does not exist or cannot be read.
3. Flatten into `Vec<(worker_id: String, line: String)>`, preserving per-worker order.
4. Cap the total to `LOG_TAIL_LINES` (currently 10, may need increasing for this panel).

### AppState field

```rust
agent_log_lines: Vec<(String, String)>, // (worker_id, line)
```

## Rendering

### Color palette

Workers are assigned a color by index (cycling if > palette size):

```
[w-0] → Cyan
[w-1] → Green
[w-2] → Magenta
[w-3] → Yellow
[w-4] → Blue
[w-5+] → cycle from Cyan
```

The `[w-N]` prefix is rendered in the worker's color; the log line content is plain white.

### Empty state

When no workers are active, the panel shows:

```
No active workers
```

in `DarkGray`.

### Long lines

Lines wider than the panel are truncated with no wrapping (consistent with the existing events panel behavior).

## Code changes

All changes are in `src/cli/status_live.rs`:

| Change | Details |
|---|---|
| `AppState` struct | Add `agent_log_lines: Vec<(String, String)>` |
| `AppState::load` | Read log files for active workers, populate `agent_log_lines` |
| `draw_body` | Split the events row into two columns (50/50); call `draw_log` on left and new `draw_agent_logs` on right |
| `draw_agent_logs` (new fn) | Render `agent_log_lines` as a `List` with colored worker-ID prefixes |
| Tests | Add smoke test with populated `agent_log_lines`; add unit test for the load logic with a temp log file |

## Testing

- **Smoke test** (`status_live_draw_smoke_populated`): extend existing test to include `agent_log_lines` with sample data; assert the draw call completes without panic.
- **Unit test** (`agent_log_lines_loads_from_file`): create a temp dir with a fake `.acs/logs/w-0.log`, register `w-0` as working, call `AppState::load`, assert `agent_log_lines` contains the expected lines prefixed with `"w-0"`.
- **Empty state test**: no active workers → `agent_log_lines` is empty.

## Non-goals

- No scrolling/keyboard navigation within the log panel (static tail view only).
- No filtering by worker.
- No log parsing or colorization of log content (raw lines only).
