# Background
ACS is a Rust CLI that executes tickets in isolated git worktrees under `.acs/worktrees/` and persists orchestration state in `.acs/project.db`.

Operators need fast, scriptable visibility into “what’s wrong right now” so the manager/worker loop can be restarted or repaired before tickets get stuck or worktrees accumulate.

# Problem
There is currently no single command that answers, in a structured way:
1. Is `.acs/project.db` reachable and writable (and not locked)?
2. Are any `in_progress` tickets effectively stuck beyond the worker timeout?
3. Do we have orphaned directories under `.acs/worktrees/` that are not registered as active git worktrees?
4. Is the git repository state safe for automatic merges (no merge-in-progress / conflicts)?
5. Are blocked tickets truly blocked by an incomplete blocker, or are they stale (blocker completed/missing)?

# Questions and Answers
Q1. What is the definition of “stuck”?
-> A: Tickets with `status = in_progress` whose `updated_at` age exceeds `config.manager.worker_timeout_seconds`.

Q2. What counts as “truly blocked”?
-> A: `tickets.status = blocked` with `blocked_by` pointing to an existing ticket whose `status != completed`.

Q3. What counts as “stale”?
-> A: Any `blocked` ticket that lacks `blocked_by`, points to a missing ticket, or points to a ticket that is already `completed`.

Q4. How should `acs run` behave when health checks fail?
-> A: `acs run` should print a warning on startup (to stderr) if any check is `warn` or `error`, but it should still attempt to start the manager/workers.

# Design
## New CLI command: `acs health`
Add `acs health` as a Clap subcommand that runs 5 checks and prints a JSON report:

- Each check reports a `status` field: `ok`, `warn`, or `error`.
- The top-level report includes an `overall` status computed as the worst check.

Checks:
1. `db`:
   - Verify `.acs/project.db` exists and can be opened.
   - Run a trivial query (`count_by_status`) to detect lock/unreadable state.
2. `stuck_workers`:
   - Load `Config` and compute `timeout_seconds = config.manager.worker_timeout_seconds`.
   - For every ticket with `status = in_progress`, compute `now - updated_at`.
   - If any age exceeds `timeout_seconds`, return `warn` with a small list of offending tickets (id, assignee, age_secs).
3. `orphaned_worktrees`:
   - List directories under `.acs/worktrees/`.
   - Compute “active worktrees” by running `git worktree list --porcelain` in the repo root.
   - Orphans are directories not present in the active set.
   - If any orphans exist, return `warn` with the count + names.
4. `git_merge_safety`:
   - Detect merge-in-progress/unmerged paths.
   - Detect whether the working tree/index is dirty (tracked changes).
   - If there are unmerged conflicts or merge-in-progress, return `error`.
   - If there are tracked changes (or untracked files), return `warn`.
   - Otherwise `ok`.
5. `blocked_vs_stale`:
   - Load tickets with `status = blocked`.
   - For each, load its blocker ticket when `blocked_by` is set.
   - Compute `truly_blocked` and `stale` counts.
   - If both are zero, return `ok`, else `warn`.

## Startup behavior: `acs run`
- Before registering/spawning tasks, call `health` checks.
- If any check is `warn` or `error`, print a one-line warning to stderr including the JSON report (or a summarized subset).

# Implementation Plan
1. Create `src/cli/health.rs` implementing the five checks + JSON report type.
2. Wire the command into:
   - `src/cli/mod.rs` (new module + `Commands::Health` variant)
   - `src/main.rs` dispatch match
3. Update `src/cli/run.rs` to call health checks during startup and print a warning when non-`ok`.
4. Add unit tests in `src/cli/health.rs` for:
   - `stuck_workers` classification using in-memory DB + `timeout_seconds = 0`.
   - `blocked_vs_stale` classification using in-memory DB.
   - Parsing helpers (and/or small git-state tests using a temporary repo).
5. Run `cargo test`.

# Examples
1. Run health diagnostics:
   - `acs health`
2. Observe startup warning:
   - `acs run --workers 2`
   - On issues, `acs run` prints `Health warning ...` to stderr before continuing.

# Trade-offs
1. “Stuck” uses `tickets.updated_at` rather than `agents.last_heartbeat` because:
   - Worker timeout changes ticket status deterministically.
   - `last_heartbeat` is only updated on DB writes by the worker/manager, and not continuously.
2. Git worktree orphan detection depends on `git` commands:
   - If git is unavailable, the check will return `error` (and overall becomes `error`).

# Implementation Results
TBD (filled after implementation).

