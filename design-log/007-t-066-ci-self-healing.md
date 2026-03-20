# 007 - t-066 CI self-healing loop (post-merge regression ticketing)

## Background
ACS merges completed worker branches into `main` to enable scoring and progress.
Currently, if a merge introduces a CI regression (e.g., `cargo test` fails on `main`), the feedback loop is delayed and requires human intervention.

## Problem
After branch merges in `manager.rs`, we need an automated mechanism to:
1. Run `cargo test` against the merged state.
2. If it fails, auto-create a high-priority (P1) fix ticket with a concise failure summary.
3. Log a `ci_regression` event for observability.

## Questions and Answers
Q1. Where in the manager lifecycle should this run?
-> A: In `src/manager.rs`, immediately after a successful git merge on the `branch_merged` path.

Q2. How do we disable the behavior safely?
-> A: Gate the behavior behind `CI_CHECK_AFTER_MERGE=1` so default behavior remains unchanged.

Q3. What failure summary should be stored on the ticket?
-> A: A truncated string (max 500 chars) derived from captured `cargo test` `stderr` (fallback to `stdout`), prefixed with the exit context.

Q4. How do we avoid blocking the manager loop?
-> A: Spawn the CI check in the background (tokio task + `spawn_blocking` for the blocking `Command` call).

## Design
### New function
Add `post_merge_ci_check()` to `src/manager.rs` and call it after the merge succeeds.

Behavior:
1. Read `CI_CHECK_AFTER_MERGE` from environment.
2. If not equal to `"1"`, return immediately.
3. Spawn a background task that:
   - Runs `cargo test` via `std::process::Command` in the ACS project directory.
   - Captures `stdout` and `stderr`.
   - On failure:
     - Builds a ticket title: `Fix CI regression after merging <branch>: <failure summary>`.
     - Creates a P1 ticket via `db.create_ticket()` using the truncated error summary (<= 500 chars).
     - Logs a `ci_regression` event.

### Failure summary formatting
Create a helper (pure function) like:
`fn summarize_ci_failure(exit_code: Option<i32>, stdout: &str, stderr: &str) -> String`

Rules:
- Prefer `stderr`, fallback to `stdout`.
- Trim, collapse whitespace, and cap to 500 chars.
- Include exit context (e.g., `exit_code=<n>` or `signal=<...>`) when available.

## Implementation Plan
1. Implement `post_merge_ci_check()` in `src/manager.rs`.
2. Identify the `branch_merged` path and call `post_merge_ci_check()` immediately after the merge succeeds.
3. Wire `db.create_ticket()` with P1 priority and truncated summary (<= 500 chars).
4. Add `ci_regression` event logging through the existing DB/events API.
5. Add unit tests:
   - `summarize_ci_failure` truncation and fallback behavior.
   - `CI_CHECK_AFTER_MERGE` gating (pure env check) if structured accordingly.
6. Run `cargo test`.

## Examples
✅ Ticket title format:
- `Fix CI regression after merging acs/t-123-1a2b: exit_code=101 cargo test failed: <first error>`

❌ No ticket when CI gate is disabled:
- `CI_CHECK_AFTER_MERGE` unset or not `"1"` -> no `cargo test` run, no ticket, no event.

## Trade-offs
1. Runtime overhead:
   - Background CI checks increase CPU usage after merges. Gate defaults to disabled to avoid unintended load.
2. Failure summary quality:
   - Truncation to 500 chars improves ticket readability but may omit deeper context; operators can inspect CI logs separately.
3. Flakiness:
   - If `cargo test` is flaky, repeated P1 tickets may be created; we rely on current ticket dedup behavior (if any).

## Implementation Results
TBD (filled after implementation).

