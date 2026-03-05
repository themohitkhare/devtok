# Synapse SpacetimeDB Backend

SpacetimeDB Rust module for **Synapse** — a TikTok-style AI agent monitoring app. This module defines the schema and reducers for projects, agents, action cards, feedback, and concurrent tasks.

## Prerequisites

- **Rust** (via [rustup](https://rustup.rs/) recommended): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Wasm target**: `rustup target add wasm32-unknown-unknown`
- **SpacetimeDB CLI**:  
  `curl --proto '=https' --tlsv1.2 -sSf https://install.spacetimedb.com | sh`  
  or: `cargo install spacetimedb-cli`  
  Ensure `spacetime` is on your `PATH` (e.g. `~/.local/bin`).

## Build

From this directory:

```bash
spacetime build
```

Compiles the module to WebAssembly. For smaller/faster modules, install [binaryen](https://github.com/WebAssembly/binaryen) and `wasm-opt`; the CLI will use it if available.

## Run locally

1. **Start the local SpacetimeDB server** (in a separate terminal):

   ```bash
   spacetime start
   ```

2. **Publish the module** to the local instance:

   ```bash
   spacetime publish synapse
   ```

   This creates (or updates) a database named `synapse` and deploys this module.

3. **View module logs**:

   ```bash
   spacetime logs synapse
   ```

## Schema summary

| Table            | Purpose                                      |
|------------------|----------------------------------------------|
| `project`        | Projects (id, name, status, repository_path, created_at) |
| `agent`          | AI agents (id, name, specialty, avatar_seed, last_seen) |
| `action_card`    | Agent action cards (status, visual_type, content, task_summary, priority, …) |
| `feedback`       | Feedback on cards (approve, reject, comment, escalate) |
| `concurrent_task`| Per-agent concurrent tasks (task_type, status, color) |

## Reducers

- `create_agent`, `insert_action_card`, `approve_action`, `reject_action`
- `add_comment`, `escalate_action`, `update_agent_status`
- `insert_concurrent_task`, `complete_task`
- `seed_demo_data` — inserts 3 agents and 5 action cards for demo use

Call `seed_demo_data` after publishing to populate sample data.
