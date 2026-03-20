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

## Signature Changes

`status_live::run` gains an `acs_dir: &Path` parameter (the `.acs/` directory):

```rust
pub fn run(db: &Db, acs_dir: &Path) -> Result<()>
```

`AppState` stores the path so `load` can derive the log directory:

```rust
struct AppState {
    // existing fields ...
    acs_dir: PathBuf,
    agent_log_lines: Vec<(String, String)>, // (worker_id, line)
}
```

The call site in `src/cli/status.rs` already resolves `acs_dir` — it simply passes it through to `run`. The existing smoke tests that construct `AppState` directly must populate `acs_dir` with a temp directory.

## Data Loading

### Log file location

`.acs/logs/<worker_id>.log` — one file per worker, written by `Spawner::spawn_provider`.

### Constants

```rust
const AGENT_LOG_TAIL_LINES: usize = 10; // separate from LOG_TAIL_LINES used for DB events
```

Both constants start at 10 but are independent and may diverge.

### Loading logic (in `AppState::load`)

1. Filter `agents` to those with `status == "working"`.
2. For each active worker, read the tail of its log file:
   - Use `std::fs::read` + `String::from_utf8_lossy` to handle any invalid UTF-8 bytes in subprocess output without silently dropping the file.
   - Split on newlines and take the last `AGENT_LOG_TAIL_LINES / max(1, active_count)` lines (minimum 1), using a fixed constant — not a runtime panel height, which is unavailable at load time.
   - Skip silently if the file does not exist or cannot be read (e.g. worker just started).
3. Flatten into `Vec<(worker_id: String, line: String)>`, preserving per-worker order, and cap the total to `AGENT_LOG_TAIL_LINES`.

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

When no workers are active (or no log lines were read), the panel shows:

```
No active workers
```

in `DarkGray`.

### Long lines

Lines wider than the panel are truncated without wrapping, consistent with the existing events panel.

## Code Changes

All changes are in `src/cli/status_live.rs` and the single call site in `src/cli/status.rs`:

| Location | Change |
|---|---|
| `src/cli/status.rs` | Pass `&acs_dir` to `status_live::run` |
| `AppState` struct | Add `acs_dir: PathBuf` and `agent_log_lines: Vec<(String, String)>` |
| `AppState::load` | Accept `acs_dir: &Path`; read log files for active workers; populate `agent_log_lines` |
| `draw_body` | Introduce a horizontal 50/50 split of `body[1]`; render existing `draw_log` (Recent Events) in `split[0]`; render new `draw_agent_logs` in `split[1]` |
| `draw_agent_logs` (new fn) | Render `agent_log_lines` as a `List` with colored `[w-N]` prefixes and a `" Agent Logs "` block title |
| Tests | Update existing smoke tests to supply `acs_dir`; add new unit tests described below |

## Testing

- **Smoke test (empty state)** — extend `status_live_draw_smoke_empty_state` to pass a temp `acs_dir`; assert draw completes without panic.
- **Smoke test (populated)** — extend `status_live_draw_smoke_populated` to write a fake `.acs/logs/w-0.log`, include it in `agent_log_lines`, and assert draw completes.
- **Unit test: log loading** — create a `tempfile::TempDir` with `.acs/logs/w-0.log` containing known lines; register `w-0` as working; call `AppState::load(db, tmp.path().join(".acs"))` ; assert `agent_log_lines` contains the expected `("w-0", line)` pairs.
- **Unit test: empty when no active workers** — no workers with `status == "working"`; assert `agent_log_lines` is empty.
- **Unit test: missing log file is skipped** — worker is `working` but log file does not exist; assert `agent_log_lines` is empty, no panic.

## Non-Goals

- No scrolling or keyboard navigation within the log panel (static tail only).
- No filtering by worker.
- No parsing or syntax-highlighting of log content (raw lines only).
- **Large log files:** the implementation reads the entire file into memory before taking the tail. For workers that have been running for hours, files may be large. This is acceptable for the initial implementation; a seek-from-end optimisation is a future improvement.
