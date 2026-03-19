# Synapse OS — Autonomous Project Management System
**Date:** 2026-03-19
**Status:** Approved

---

## Overview

Synapse OS evolves the existing Synapse TikTok-style agent monitoring UI into a fully autonomous project management system. Given a GitHub/GitLab repository or a written spec, the system bootstraps a virtual dev team of AI agents — each with a distinct role (PM, Tech Lead, Frontend Dev, Backend Dev, QA, DevOps) — and drives the project to completion with minimal human intervention. The human's role is oversight: reviewing escalations and major decisions via the existing Synapse feed.

---

## Entry Points

The system accepts two entry points, which can be combined:

1. **Existing repository** — the bootstrapper agent clones the repo, analyzes the codebase (structure, languages, open issues, recent commits), and generates an initial project state: backlog tickets, architecture summary, and agent assignments.
2. **Spec/brief** — a plain-language document (PRD, idea description, feature list) is passed to the manager agent, which creates epics, breaks them into tickets, and initializes the work queue.

Both modes produce the same output: a populated Project Brain and a ready-to-run agent team.

---

## Architecture

### Components

#### 1. Process Orchestrator (Python daemon)

A lightweight daemon responsible for process lifecycle, not project intelligence:

- Spawns and kills Claude Code / Cursor CLI subprocesses
- Monitors process health; restarts crashed agents
- Drives the cron tick engine (standup cadence, meeting triggers, timeout detection)
- Routes agent stdout/stderr into SpacetimeDB ActionCards visible in Synapse
- Manages agent registration and heartbeat system
- Watches SpacetimeDB `feedback` table for human decisions on escalation cards and forwards them to the relevant manager's Redis inbox (`manager_inbox:{id}`)

The orchestrator has no project awareness — it does not make decisions about what to work on. That responsibility belongs entirely to manager agents.

#### 2. Manager Agents (1–N instances)

Each manager is a Claude Code CLI process with a specialized system prompt and an extended tool set. Managers own a **domain** (e.g. Frontend, Backend, DevOps, QA) and are responsible for:

- Creating and maintaining tickets within their domain
- Assigning tickets to workers (via worker inbox) or posting to the shared work queue
- Running standups and interpreting worker status
- Calling design reviews before major implementation decisions
- Performing or delegating PR reviews
- Syncing with peer managers on cross-domain dependencies
- Escalating to human when decisions exceed their authority

**Multiple manager support:** Each manager is an independent process. They communicate via the Project Brain (Redis). An optional **Director Agent** (also a Claude Code instance) can coordinate between managers, resolve conflicts, and make cross-domain prioritization calls. The Director is optional — for small projects, one manager is sufficient.

**Manager authority boundaries:** A manager may autonomously:
- Create, assign, and close tickets within their domain
- Merge PRs on non-main branches (feature branches, domain-specific staging branches)
- Write to the knowledge base for keys within their domain prefix (e.g. `frontend:*`, `backend:*`)
- Spawn and kill worker agents within their domain allocation

A manager **must** call `request_human_approval` for:
- Merges to `main`/`master` or any protected branch
- Introduction of new external service dependencies (APIs, third-party packages not already in the repo)
- Architecture changes that affect multiple domains
- Any decision with irreversible external effect (deploys, data migrations, published API changes)
- Spending decisions (e.g. paid API usage above a configurable threshold)

Manager tool set (MCP tools exposed to the Claude Code process):
- `create_ticket(title, description, domain, priority, assignee?)` — creates ticket in Redis, adds to work queue or directly to assignee inbox
- `assign_ticket(ticket_id, agent_id)` — pushes ticket to worker's inbox directly (bypasses work queue)
- `update_ticket(ticket_id, status, notes)` — writes to `tickets:{id}` hash
- `read_knowledge_base(key_prefix)` / `write_knowledge_base(key, value, expected_version?)` — see Knowledge Base section for conflict resolution
- `send_agent_message(agent_id, message)` — RPUSH to `agent_inbox:{id}`
- `call_standup(timeout_seconds?)` — collects statuses from `standup_responses:{manager_id}` list, default timeout 120s
- `call_meeting(participants, agenda, timeout_seconds?)` — initiates structured exchange via participant inboxes, default timeout 300s
- `request_human_approval(decision, context, urgency, timeout_hours=24)` — creates high-priority Synapse card via SpacetimeDB; polls `manager_inbox:{id}` for orchestrator-forwarded human response; on timeout, takes the safe default action specified in `decision.safe_default` (e.g. "do not merge", "do not deploy") and posts a `ESCALATION_TIMEOUT` ActionCard
- `open_pr(branch, title, description)` / `review_pr(pr_id, verdict, comments)` — git/GitHub operations
- `assign_pr_review(pr_id, reviewer_agent_id)` — pushes PR review request to reviewer's inbox

#### 3. Project Brain (Redis)

The shared persistent memory layer. All agents (managers and workers) read from and write to this store. Structure:

```
work_queue                  List       — Tickets ready to be claimed (RPUSH/BLPOP, FIFO with priority)
in_progress                 Hash       — ticket_id → agent_id (written atomically via Lua script, see below)
agent_inbox:{id}            List       — Per-worker message queue (RPUSH/BLPOP); used for ticket assignments and manager messages
manager_inbox:{id}          List       — Per-manager message queue (RPUSH/BLPOP); receives worker completions, orchestrator events, human escalation responses
agent_state:{id}            Hash       — Status, current ticket, last heartbeat (used by all agent types including managers)
standup_responses:{mgr_id}  List       — Workers RPUSH status blobs here; manager BLPOPs to collect
knowledge_base:{domain}:{k} String     — Versioned key-value (see Knowledge Base section)
knowledge_base_ver:{k}      String     — Integer version counter for each key
meeting:{id}                List       — Temporary per-meeting message list (TTL: 1h after last write; ID is UUID assigned by call_meeting)
meeting_log                 List       — Persistent transcript entries (JSON: timestamp, speaker, content, meeting_id)
project_state               Hash       — Phase, health score, active blockers, metrics
tickets:{id}                Hash       — Full ticket data (title, desc, status, assignee, domain)
orchestrator_events         List       — Orchestrator RPUSH-es system events (agent_crashed, bootstrap_complete, etc.); orchestrator main loop BLPOPs this
```

**Atomic ticket claiming:** Workers claim a ticket from `work_queue` using `BLPOP` (which is atomic — only one worker receives any given ticket). Immediately after receiving the ticket ID, the worker runs a Lua script to atomically write the claim to `in_progress` and update `tickets:{id}.status` to `in_progress`. This two-step is safe because BLPOP guarantees exclusive delivery — no second worker can claim the same ticket.

**Assignment model:** Two flows exist and are not contradictory:
- **Pull (default):** Manager adds ticket to `work_queue`; any available worker BLPOP-claims it
- **Push (explicit):** Manager uses `assign_ticket(ticket_id, agent_id)` to RPUSH the ticket directly to a specific worker's `agent_inbox:{id}` and skips the work queue. Used when domain expertise is required or when a manager wants a specific agent to handle sensitive work.

**Knowledge base conflict resolution:** Knowledge base keys are versioned. To write:
1. Read current version from `knowledge_base_ver:{key}`
2. Use a Lua script to conditionally set `knowledge_base:{domain}:{key}` only if `knowledge_base_ver:{key}` equals the version read (optimistic locking)
3. If version mismatch (another manager wrote concurrently), re-read and retry or escalate via manager sync

Domain prefixes enforce soft ownership: each manager owns their domain prefix. Cross-domain keys (e.g. `api_contract:auth`) require the writing manager to first complete a manager sync (see Interaction Types).

#### 4. Worker Agents (N instances)

Workers are Claude Code or Cursor CLI processes with a focused tool set. They:

1. Block on their inbox (`agent_inbox:{id}`) for a ticket assignment (BLPOP), OR pull from `work_queue` if no inbox message within a configurable poll interval
2. Read the ticket and relevant knowledge base entries
3. Execute the work (write code, tests, config) against the target repository
4. Post progress ActionCards to Synapse via SpacetimeDB
5. Open a PR when work is complete; RPUSH completion message to `manager_inbox:{manager_id}` including the PR URL and ticket ID
6. Update `tickets:{id}.status` to `review_pending`

Workers have the following MCP tools: file read/write, bash, git operations, `post_action_card(type, content)` (writes to SpacetimeDB), `read_knowledge_base(key_prefix)`, and `update_ticket_status(ticket_id, status)` (scoped — workers may only set status to `in_progress`, `review_pending`, or `blocked`; full `update_ticket` with notes remains manager-only). Workers do not have project management tools beyond these.

**PR review assignment:** On PR open, the worker's completion message to the manager inbox includes the PR URL. The manager calls `assign_pr_review(pr_id, reviewer_agent_id)` to push a review request to the appropriate agent's inbox. The reviewer (manager or QA worker) uses `review_pr()` to post verdict and comments. QA worker agents have `review_pr()` added to their tool set. Default reviewer selection: QA agent if one is active in the domain; otherwise the domain manager.

#### 5. Bootstrap Agent

The bootstrapper is a **dedicated manager instance** running a bootstrap-mode system prompt. It has read-only access to the target repository plus `create_ticket` and `write_knowledge_base` tools. It does not have `assign_ticket` or worker management tools — its only job is initial project setup.

**Completion signal:** When bootstrap is complete, the bootstrapper calls `write_knowledge_base("system:bootstrap_complete", {ticket_count, domains, timestamp})` and then exits (the Claude Code process naturally terminates). The orchestrator watches for either (a) the process to exit cleanly, or (b) a `bootstrap_complete` entry in `project_state` (set by the Lua write above), whichever comes first. On detection, the orchestrator spawns domain managers and workers.

In a multi-manager setup, tickets created by the bootstrapper include a `domain` field. The orchestrator routes each ticket to the appropriate `work_queue:{domain}` (Phase 3) or to a single `work_queue` (Phase 2). This routing logic is in the orchestrator, not the bootstrapper — the bootstrapper just sets the domain field on each ticket.

**Phase 2 → Phase 3 queue migration:** In Phase 2, all tickets go into a single `work_queue`. In Phase 3, the orchestrator moves to per-domain queues (`work_queue:{domain}`). The migration is handled at Phase 3 startup: the orchestrator drains `work_queue`, reads each ticket's domain field, and re-enqueues into `work_queue:{domain}`. The `tickets:{id}` hash is unchanged — only the queue routing changes.

#### 6. SpacetimeDB (existing)

Unchanged from current Synapse. All ActionCards from all agents (workers and managers) flow into SpacetimeDB. New card types added:

- `STANDUP_SUMMARY` — aggregated standup output from manager
- `MEETING_TRANSCRIPT` — full or summarized design review transcript
- `TICKET_CREATED` / `TICKET_ASSIGNED` / `TICKET_COMPLETED`
- `PR_OPENED` / `PR_REVIEW` / `PR_MERGED`
- `ESCALATION` — high-priority human approval request
- `PROJECT_COMPLETE` — posted by manager when all tickets are closed

The orchestrator watches the SpacetimeDB `feedback` table (via WebSocket subscription) for human responses. Two card types trigger forwarding:
- **`ESCALATION`** — response (approve/reject/comment) is RPUSH-ed to the relevant manager's `manager_inbox:{id}`, unblocking `request_human_approval`
- **`PROJECT_COMPLETE`** — an approve gesture signals project sign-off; orchestrator receives this via the same watcher and initiates graceful shutdown of all agent processes; reject/comment causes the manager to re-evaluate completion state and potentially re-open tickets

The `feedback` entry includes the `card_id`, which maps to the originating manager's agent ID via a `escalation_card:{card_id}` → `manager_id` lookup key in Redis (written when `request_human_approval` creates the card).

#### 7. Synapse Feed (existing React UI, evolved)

The human's window into the system. Additions in Phase 4:

- Manager agents have distinct visual profiles (higher-tier orbital rings, PM badge)
- Meeting cards show collapsible transcripts
- Ticket board view (kanban) as a second view mode alongside the feed
- Project health dashboard (velocity, blockers, agent activity heatmap)
- Multi-manager view showing domain breakdowns

---

## Interaction Types

### Standup (Daily Cron)
- Orchestrator triggers standup tick on schedule (default: every 24h, configurable)
- On tick, orchestrator RPUSH-es a standup-request message to each active worker's `agent_inbox:{id}`: `{type: "standup_request", manager_id, deadline_epoch}`
- Each active worker reads the standup-request from its inbox and RPUSH-es a structured status blob to `standup_responses:{manager_id}`: `{agent_id, did, doing, blocked}`
- Orchestrator also RPUSH-es a `{type: "standup_tick"}` message to the domain manager's `manager_inbox:{id}` to signal that standup collection should begin
- Manager calls `call_standup(timeout_seconds=120)`, which BLPOP-collects from `standup_responses:{manager_id}` until timeout or all expected responses received (expected count read from `project_state.active_worker_count`)
- Workers who don't respond within the timeout are flagged as potentially unresponsive; their heartbeat in `agent_state:{id}` is checked
- Manager synthesizes responses, reprioritizes `work_queue` if needed, and posts `STANDUP_SUMMARY` ActionCard

### Design Review (On Demand)
- Manager calls `call_meeting(participants, agenda, timeout_seconds=300)` before major architectural decisions
- Orchestrator delivers meeting invite to each participant's `agent_inbox:{id}`
- Participants exchange structured messages (RPUSH to a temporary `meeting:{id}` list, BLPOP to read responses) in turn-based fashion mediated by the manager
- On timeout or explicit close, manager writes decisions to knowledge base and posts `MEETING_TRANSCRIPT` card

### One-on-One (Triggered)
- Triggered when a worker's `agent_state:{id}.status` shows `blocked` for longer than a configurable threshold (default: 30 min), detected by orchestrator health check
- Orchestrator notifies domain manager via `manager_inbox:{id}`
- Manager sends direct message to worker inbox with context and suggestions
- Worker responds with updated status or escalation request
- If unresolved within another threshold (default: 15 min), manager calls `request_human_approval`

### PR Review (Continuous)
- Worker opens PR, updates ticket status, RPUSH-es completion to `manager_inbox:{id}`
- Manager calls `assign_pr_review(pr_id, reviewer_agent_id)` — RPUSH to reviewer inbox
- Reviewer uses `review_pr()` to post verdict and comments (written to SpacetimeDB as `PR_REVIEW` card; comments also RPUSH-ed to worker inbox)
- Worker picks up comments, iterates; repeats until approved
- On approval, PR merges; manager marks ticket complete in Redis and SpacetimeDB

### Manager Sync (Cross-domain)
- Triggered when a ticket has a cross-domain dependency (e.g. a frontend ticket requires a backend API that isn't built yet)
- Relevant managers exchange messages via their inboxes using `send_agent_message`
- Resolution: write agreed API contract to shared knowledge base key with domain-neutral prefix (e.g. `contract:auth_api`), unblock dependent ticket, or reassign
- Cross-domain knowledge base writes require both managers to complete the sync first (manager writing the key includes the sync transcript reference in the value)
- If managers conflict, optional Director Agent is messaged for arbitration; if no Director, escalated to human

### Human Escalation
- Manager calls `request_human_approval(decision, context, urgency, timeout_hours=24)`:
  1. Tool writes `escalation_card:{card_id}` → `manager_id` to Redis
  2. Creates `ESCALATION` ActionCard in SpacetimeDB (high-priority)
  3. Manager process polls `manager_inbox:{id}` (BLPOP with 30s timeout, retrying) until response arrives or `timeout_hours` elapses
- Synapse shows `ESCALATION` card with full context; human approves or rejects via existing gesture interface
- Orchestrator's SpacetimeDB feedback watcher detects the feedback entry, looks up `manager_id` via `escalation_card:{card_id}`, and RPUSH-es `{approved: bool, comment: str, card_id}` to `manager_inbox:{id}`
- Manager's BLPOP returns with the decision; manager proceeds accordingly
- **On timeout:** Manager takes `decision.safe_default` action (e.g. "do not merge"), posts `ESCALATION_TIMEOUT` ActionCard, and continues

---

## Project Lifecycle

```
Bootstrap
  ↓
  Human provides: repo URL and/or spec document
  Orchestrator spawns bootstrapper agent (dedicated manager with read-only + create_ticket tools)
  Bootstrapper analyzes repo/spec → creates initial tickets → writes project_state
  Bootstrapper posts TICKET_CREATED cards to Synapse for visibility
  Human reviews initial plan via Synapse (optional approval gate via ESCALATION card)
  Orchestrator terminates bootstrapper, spawns domain managers and workers
  ↓
Execution Loop (continuous Kanban)
  ↓
  Workers pull tickets from work_queue (or receive via inbox assignment)
  Workers code, test, open PRs
  Manager monitors via standup, unblocks one-on-ones, reviews PRs
  New tickets created as dependencies are discovered
  ↓
Sync Points (cron + event-driven)
  ↓
  Daily standups
  Design reviews before major features
  One-on-ones for blocked agents
  Manager syncs for cross-domain work
  Human escalations for decisions outside manager authority
  ↓
Delivery
  ↓
  All tickets closed → manager detects via project_state → posts PROJECT_COMPLETE card
  Human reviews, signs off (gesture on PROJECT_COMPLETE card)
  Orchestrator terminates all agent processes
```

---

## Data Flow

```
Spec/Repo → Bootstrap Agent → Project Brain (tickets, knowledge)
                                    ↓
                            Manager Agent(s)
                           (reads queue, assigns)
                                    ↓
                            Worker Agents
                          (claim, code, open PR)
                                    ↓
                            SpacetimeDB (ActionCards)
                                    ↓
                            Synapse Feed (human)
                                    ↓
                  Human feedback → SpacetimeDB feedback table
                                    ↓
                  Orchestrator watches → Redis RPUSH → Manager inbox
```

---

## Implementation Phases

### Phase 1 — Process Orchestrator + Project Brain
- Redis schema, key conventions, and a shared Python client library
- Process orchestrator daemon: spawn/kill CLI subprocesses, health checks, crash recovery
- Cron tick engine (standup trigger, blocker detection for one-on-one)
- Stdout routing from CLI processes → SpacetimeDB ActionCards
- Agent registration and heartbeat system (`agent_state:{id}`)
- SpacetimeDB feedback watcher → Redis forwarding

### Phase 2 — Bootstrap + Single Manager MVP
- Bootstrapper agent: system prompt, repo/spec analysis tools, ticket creation flow
- Single manager working end-to-end: create tickets, assign workers, collect standups, unblock
- Manager tool MCP server (all manager tools listed above)
- Basic standup loop and one-on-one trigger

### Phase 3 — Multi-Manager + Meeting System
- Multiple manager support: domain ownership, domain-prefixed knowledge base, `work_queue:{domain}` per-domain queues
- Director agent (optional): inter-manager sync arbitration
- Design review protocol: temporary meeting lists, turn-based exchange
- PR review workflow: auto-assignment, comment loop, merge trigger
- Inter-manager sync protocol with knowledge base conflict resolution

### Phase 4 — Synapse UI Evolution
- New ActionCard types: standup summary, meeting transcript, ticket events, PR events, PROJECT_COMPLETE
- Manager visual profiles in the feed (PM badge, distinct orbital ring style)
- Kanban board view (second mode alongside feed)
- Project health dashboard
- Multi-manager domain view

---

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Redis for Project Brain | Low-latency, native list/hash primitives match access patterns. BLPOP enables efficient worker blocking without polling. AOF for durability. |
| CLI subprocesses (not API) | Agents are full Claude Code / Cursor sessions with file system access, git, bash — capabilities unavailable via raw API. |
| SpacetimeDB unchanged | Existing reactive subscription infrastructure works for ActionCard fan-out. No reason to replace. |
| Manager as Claude Code instance | Manager intelligence IS the LLM — system prompt defines role, MCP tools define capabilities. No custom framework needed. |
| Lua scripts for atomic claims | Redis Lua scripts execute atomically, preventing split-brain between BLPOP and `in_progress` write. |
| Optimistic locking for knowledge base | Avoids write locks while still preventing silent overwrite. Retry is cheap; conflicts are rare in practice. |
| Orchestrator as SpacetimeDB watcher | Keeps the feedback loop (human gesture → agent inbox) clean without adding a REST polling layer to manager agents. |
| Optional Director Agent | Avoids premature complexity for small projects. Additive when needed. |

---

## Out of Scope (v1)

- Cloud deployment / multi-machine agent distribution
- Custom LLM fine-tuning for agent roles
- Agent memory beyond the current project (cross-project learning)
- GUI for configuring agent teams (CLI/config file for v1)
- Billing / resource metering for CLI token usage
- Automated deployment pipelines (agents open PRs; humans trigger deploys via escalation approval)
