# Background
ACS (Auto Consulting Service) is a Rust CLI that turns a repository into a self-developing system by:
- Bootstrapping a backlog of tickets (via an LLM “bootstrap agent” that calls `acs ticket create` and `acs kb write`).
- Spawning a manager + N worker agents (`acs run`) that execute tickets in isolated git worktrees using the Claude Code CLI.
- Merging worker branches into `main` when tickets complete.

Users want ACS to be able to develop itself repeatedly via a self-development loop and to add “missing features” needed for that workflow.

# Problem
Currently:
1. `acs run` is infinite and requires `Ctrl+C`. There is no way to run the manager/worker system until the queue is empty or until a bounded number of cycles/time.
2. There is no command that iterates “execute tickets -> re-analyze repository -> add more tickets -> repeat”.
3. The bootstrap prompt examples do not explicitly pass `--non-interactive` to `acs ticket create`. Self-development iterations are likely to hit dedup matches and could cause the bootstrap agent to wait for stdin (since `ticket create` will prompt for confirmation unless `--non-interactive` is set).
4. Shutdown currently relies on a watch signal checked only between polling cycles. If a worker is actively waiting on a Claude subprocess, the shutdown signal may not stop quickly, which is required for “bounded runs” inside a loop.

# Questions and Answers
Q1. What does “self development loop” mean operationally?
-> A: Iteratively run the ACS manager/workers on the current backlog, then re-run an LLM “bootstrap” (incremental mode) to add new tickets based on the updated repository + knowledge base, and stop when the system is stable (no new work added) or the max iteration count is reached.

Q2. How do we avoid infinite loops?
-> A: Add guardrails:
     - Cap iterations with `--max-iterations`.
     - Stop early if ticket count does not increase after a post-run bootstrap.
     - Provide an optional `--max-run-seconds` and/or stop when no tickets remain in `pending`, `in_progress`, or `review_pending`.

Q3. How do we prevent bootstrap ticket dedup prompts from blocking?
-> A: Update bootstrap prompt examples to call:
     - `acs ticket create ... --non-interactive`

# Design
## New CLI command
Add `acs evolve` (or `acs devloop`) with the following default behavior:
1. Ensure `.acs/` exists (fail fast if not).
2. For `iteration = 1..max_iterations`:
   1. Optionally run the architect planner (`acs plan`) if `--plan-each-iteration` is set.
   2. Start manager + workers bounded until the queue is empty (no `pending`, `in_progress`, or `review_pending`), or until `--max-run-seconds` is reached.
   3. Optionally run an incremental bootstrap agent to add more tickets (`--bootstrap-after-run` default true).
   4. Compare ticket counts before vs after bootstrap; if unchanged and `--stop-when-no-new-tickets` is set (default true), break.

## Bounded run mode (implementation approach)
Implement bounded evolution by:
- Spawning `manager::run_loop` and `worker::worker_loop` exactly like `acs run` does.
- Polling the SQLite DB for ticket counts in the loop.
- Sending the existing `shutdown_tx` to end manager and workers when the stop condition is met.

## Worker shutdown responsiveness (required for bounded runs)
Modify `worker::handle_ticket_assignment` to listen to shutdown while waiting on the Claude subprocess:
- On shutdown:
  - kill the Claude process (best-effort),
  - set the ticket back to `pending` (and clear assignee),
  - set the agent back to `idle`,
  - remove the worktree (best-effort),
  - exit early.

## Incremental bootstrap prompt
Add a new prompt generator (or extend existing bootstrap prompt) to instruct the bootstrap agent to:
- Read current tickets and knowledge base entries.
- Create only new tickets that are missing/uncovered.
- Use `--non-interactive` for all `ticket create` calls.

# Implementation Plan
1. Add `design-log` file (this file) and request approval.
2. Implement `acs evolve`:
   - `src/cli/evolve.rs` new module
   - Update `src/cli/mod.rs` and `src/main.rs` to dispatch it.
3. Add prompt updates:
   - Update `bootstrap_prompt` examples to include `--non-interactive`
   - Add `incremental_bootstrap_prompt` (preferred) for loop iteration behavior
4. Implement bounded execution:
   - In `acs evolve`, start manager + worker tasks, poll stop condition, then send shutdown.
5. Improve worker shutdown:
   - Update `src/worker.rs` to react to shutdown while Claude subprocess is running.
6. Add tests:
   - Unit test helper `should_stop_evolution` (pure DB status -> bool).
   - CLI smoke tests:
     - `acs evolve` fails when `.acs/` missing.
     - `acs evolve --dry-run` / `--max-iterations 0` avoids spawning external tools (if added).

# Examples
1. Run 1 evolution iteration (generate backlog once, execute it, then bootstrap again):
   - `acs evolve --workers 2 --max-iterations 1 --no-plan`

2. Fully automated self development loop:
   - `acs evolve --workers 3 --max-iterations 5 --plan-each-iteration --bootstrap-after-run`

3. Safety guardrails:
   - `acs evolve --max-run-seconds 900 --stop-when-no-new-tickets`

# Trade-offs
1. LLM cost:
   - Each iteration may re-run bootstrap (and optionally plan). This increases token usage and wall time.
2. Correctness:
   - Stop condition is heuristic (queue empty / ticket count stable), not “no bugs remain”.
3. Dedup behavior:
   - Ticket dedup is similarity-based; it can still create near-duplicates if titles/descriptions diverge. The `--non-interactive` change prevents blocking, not duplication.

# Implementation Results
## What was implemented
1. New CLI command: `acs evolve`
   - Added as `src/cli/evolve.rs` and wired into `src/cli/mod.rs` + `src/main.rs`.
   - Supports: `--workers`, `--max-iterations`, `--plan-each-iteration`, `--bootstrap-after-run`, `--stop-when-no-new-tickets`, `--max-run-seconds`, and `--dry-run`.
   - Bounded execution drains the queue by polling the SQLite DB for `pending`, `in_progress`, and `review_pending`.

2. Incremental bootstrap prompt
   - Added `prompts::incremental_bootstrap_prompt()` in `src/prompts.rs`.
   - Updated `bootstrap_prompt()` examples to pass `--non-interactive` to `ticket create`.

3. Workers can use the ACS CLI from inside git worktrees
   - Added `src/cli/acs_dir.rs` with `resolve_acs_dir()` which walks upward to find `.acs/`.
   - Updated CLI commands (`ticket`, `kb`, `inbox`, `plan`, `run`, `status`, `log`, `cleanup`) to use the resolved `.acs/` path so that calls from `.acs/worktrees/<worker_id>` succeed.

4. Merge gate via tests (Rust)
   - Implemented `cargo test --quiet` gating inside workers when `Cargo.toml` exists in the worktree (`src/worker.rs`).
   - Workers include `tests_passed` in the inbox completion payload.
   - Manager merges only when `tests_passed == true` (`src/manager.rs`), otherwise it re-queues the ticket to `pending`.

5. Shutdown responsiveness for bounded runs
   - Updated `worker::handle_ticket_assignment` to listen to shutdown while waiting on the Claude subprocess; on shutdown it kills the process, re-queues the ticket to `pending`, sets the agent idle, and removes the worktree best-effort.

## Deviations from the design
1. Test gate location
   - Design proposed the gate before merging in the manager.
   - Implementation runs tests in the worker (because the worker worktree exists there), then passes `tests_passed` to the manager.

2. Merge conflict behavior
   - Design described blocking on merge conflicts.
   - Implementation re-queues merge-conflicted tickets back to `pending` and deletes the conflicting branch so the evolution loop can continue.

## Tests
1. Updated/added a new unit test in `src/manager.rs`:
   - Ensures tickets are re-queued when `tests_passed: false`.
2. Added an integration test:
   - `tests/integration_test.rs::test_evolve_dry_run` to validate the new command plumbing without spawning Claude.

