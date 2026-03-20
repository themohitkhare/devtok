# 006 - t-062 TB2 (DB2): Seed backend KB entries

## Background

The worker prompt in `src/prompts.rs` instructs backend workers to read KB entries for:

- `--domain backend --key stack`
- `--domain backend --key api-contracts`

At ticket assignment time, these entries were missing from the knowledge base (domain `backend`), so the manager could only preload the general/architecture KB entries (or none if the KB was empty).

## Problem

Backend workers were missing domain-specific context about:

- the backend stack used in this repository
- the internal inbox/tool contracts used for manager<->worker coordination

Because the manager only includes KB entries when they exist, missing backend entries silently remove that context from `ticket_assignment` payloads.

## Questions and Answers

### Q1: What does “backend/api-contracts” mean in this codebase?
**A:** There is no HTTP API. “api-contracts” is the internal tool/inbox contract between manager and worker:
`msg_type` values like `ticket_assignment` and `ticket_completed`, and the JSON payload fields manager/worker parse and produce.

### Q2: Where are the KB entries fetched and inserted into worker prompts?
**A:** In `src/manager.rs`:
- `build_kb_context_entries` fetches `(domain, "stack")`, `(domain, "api-contracts")`, and also general keys when present.
- the resulting `kb_context` string and `kb_entries` array are included in the `ticket_assignment` inbox payload.

### Q3: What is the actual internal inbox payload schema?
**A:** See `src/manager.rs` and `src/worker.rs`:
- Manager -> worker:
  - inbox `msg_type`: `ticket_assignment`
  - recipient: `worker_id`
  - payload (JSON string) contains: `ticket_id,title,description,domain,persona,work_type,kb_context,kb_entries` and optional `model`.
- Worker -> manager:
  - inbox `msg_type`: `ticket_completed` (or legacy `completion`)
  - recipient: `mgr`
  - payload (JSON string) contains: `ticket_id` and optional `tests_passed`, `work_type`/`provider`, and `model`.

## Design

Seed the missing KB entries so backend workers always receive domain context at assignment time.

- Write `backend/stack`
- Write `backend/api-contracts`
- Write a dedicated discovery entry `backend/worker-findings-t-062` with code references and internal contract notes

## Implementation Plan

1. Use the CLI to write KB entries:
   - `acs kb write --domain backend --key stack --value "..."`
   - `acs kb write --domain backend --key api-contracts --value "..."`
   - `acs kb write --domain backend --key worker-findings-t-062 --value "..."`
2. Verify the entries using `acs kb read`.
3. Re-run the test suite: `cargo test`.
4. Commit documentation of the KB seeding and internal contract mapping.

## Examples

Write the KB entries (illustrative):

- `acs kb write --domain backend --key stack --value "Rust single-binary CLI with Clap, Tokio, rusqlite/SQLite (WAL + foreign_keys), Serde/Serde_json, Ratatui/Crossterm"`
- `acs kb write --domain backend --key api-contracts --value "Manager->Worker inbox contract: ticket_assignment payload fields; Worker->Manager completion contract: ticket_completed payload fields; manager parsing rules for tests_passed and merge behavior"`

## Trade-offs

- Pros: No Rust code changes required; avoids risk of regressions.
- Cons: The KB values are textual summaries; they do not enforce runtime validation.

## Implementation Results

- Wrote and verified:
  - `backend/stack` (version 3)
  - `backend/api-contracts` (version 2)
  - `backend/worker-findings-t-062` (version 1)
- Re-ran tests: `cargo test` → `186 passed; 0 failed`.

