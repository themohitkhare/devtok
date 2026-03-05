# Synapse Mock Agent Worker

This is a small Python worker that generates realistic mock Synapse activity and sends it into SpacetimeDB via the HTTP reducer API. It periodically inserts `ActionCard` rows and creates `ConcurrentTask` rows for a few sample agents.

## Requirements

- Python 3.10+
- A running SpacetimeDB instance exposing the `synapse` module on `http://localhost:3000`

## Installation

From the `worker` directory:

```bash
pip install -r requirements.txt
```

## Running the worker

The recommended way is via the helper script:

```bash
./run.sh
```

This will:

- Print a banner.
- Install Python dependencies from `requirements.txt`.
- Start the async mock worker loop.

You can also run the worker directly:

```bash
python main.py
```

## What it does

- Seeds three agents via the `create_agent` reducer:
  - Frontend UI Agent (`React/TypeScript/CSS`)
  - DB Migration Agent (`PostgreSQL/Schema`)
  - Security Scanner (`Dependency Audit/CVE`)
- On a random interval between 8–15 seconds:
  - Chooses an agent.
  - Picks a visual type (`CodeDiff` or `TerminalOutput`).
  - Generates realistic mock content and a short summary.
  - Calls the `insert_action_card` reducer with that data.
  - Occasionally calls `insert_concurrent_task` to simulate parallel work.

All mock content lives in `src/generate_mock_content.py`.

# Synapse Mock Agent Worker

Python worker that simulates multiple AI CLI agents for **Synapse** (TikTok-style AI agent monitoring). It talks to SpacetimeDB over HTTP and creates agents, action cards, and concurrent tasks.

## What it does

- **On startup**: Registers 3 agents with the `synapse` module:
  - Frontend UI Agent (React/TypeScript/CSS)
  - DB Migration Agent (PostgreSQL/Schema)
  - Security Scanner (Dependency Audit/CVE)
- **Main loop** (every 8–15 seconds, random):
  - Inserts a new **ActionCard** (random agent, 60% CodeDiff / 40% TerminalOutput, realistic mock content).
  - 20% chance: inserts a **ConcurrentTask** for an agent.
  - 10% chance: completes a random running **ConcurrentTask**.

SpacetimeDB must be running with the `synapse` module loaded; the worker does not start the database.

## Install dependencies

```bash
cd /Users/mkhare/Development/devtok/worker
pip install -r requirements.txt
```

Requires Python 3.10+ and:

- `httpx` — used for direct HTTP calls to the SpacetimeDB REST API (no SpacetimeDB Python SDK on PyPI).

## Run

```bash
./run.sh
```

Or:

```bash
pip install -r requirements.txt
python main.py
```

Ensure SpacetimeDB is up at **http://localhost:3000** with the **synapse** module deployed.

## Stop

- **Ctrl+C** (SIGINT) or **SIGTERM** — worker exits after finishing the current sleep interval (graceful shutdown).

## Configuration

- **SpacetimeDB base URL**: `http://localhost:3000` (set in `main.py` as `SPACETIMEDB_BASE`).
- **Module name**: `synapse`.
- **Action interval**: 8–15 seconds (configurable via `ACTION_INTERVAL_MIN` / `ACTION_INTERVAL_MAX` in `main.py`).

## Reducer contract

The worker calls these SpacetimeDB reducers (signatures must match the `synapse` module):

- `create_agent(name, specialty, avatar_seed)`
- `insert_action_card(agent_id, status, visual_type, content, task_summary, project_id, priority, created_at, updated_at)`
- `insert_concurrent_task(agent_id, task_type, status, color, created_at)`
- `complete_task(task_id)`

If your module uses different argument order or types, adjust the `call_reducer` arguments in `main.py` and `generate_mock_content.py` accordingly.
