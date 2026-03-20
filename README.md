# ACS — Auto Consulting Service

ACS is a CLI tool that turns a software project into a self-developing system. It analyzes your repository, creates a backlog of tickets, then spins up a team of AI worker agents (powered by Claude Code) that execute those tickets in parallel using isolated git worktrees. A built-in manager loop assigns work, monitors progress, and reviews completed tickets — all orchestrated from a single `acs run` command.

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary is at target/release/acs
```

Install the latest tagged release binary:

```bash
curl -fsSL https://raw.githubusercontent.com/themohitkhare/devtok/main/scripts/install.sh | sh
```

Homebrew users can install from the formula in this repo or from the generated release asset:

```bash
brew install ./Formula/acs.rb
```

To update an existing installation in place:

```bash
acs update
acs update --check
```

For crates.io publication, the package metadata is now publish-ready, but `cargo install acs` still depends on ownership of the `acs` crate name on crates.io.

## Usage

### Use ACS on any git repository

ACS is a portable framework — it works on any git repository, not just the one it ships with.

```bash
# Clone any repo, initialize ACS, and start the AI team
git clone <your-repo-url>
cd <your-repo>
acs init
acs run
```

`acs init` will:
- Auto-detect the project name from your git remote (`origin`) or fall back to the directory name
- Create `.acs/` with a database, config, and log storage
- Add `.acs/` to `.gitignore` so orchestration state stays local

### Initialize a project

```bash
# Plain init (no auto-analysis)
acs init

# Bootstrap: analyze the repo and auto-create tickets
acs init --auto

# Or bootstrap from a spec/requirements file
acs init --spec requirements.md
```

This creates an `.acs/` directory containing the SQLite database (`project.db`), configuration (`config.toml`), and log storage.

### Run the AI team

```bash
# Start with the default number of workers (2)
acs run

# Or specify the number of parallel workers
acs run --workers 4
```

Workers pick up tickets, execute them in isolated git worktrees, and commit their changes to feature branches. The manager assigns tickets, tracks progress, and auto-reviews completed work. Press `Ctrl+C` to shut down gracefully.

### Check project status

```bash
acs status
```

Shows ticket counts by status, active agents, and total token usage.

### View the event log

```bash
# Show the last 20 events
acs log

# Follow mode (like tail -f)
acs log --follow

# Show more entries
acs log --limit 50
```

### Manage tickets directly

```bash
acs ticket list
acs ticket create --title "Add feature X" --description "..." --domain backend
acs ticket update --id t-001 --status in_progress
```

### Knowledge base

Workers share discoveries via a built-in key-value knowledge base:

```bash
acs kb write --domain backend --key stack --value "Rust, Axum"
acs kb read --domain backend --key stack
```

### Inbox (inter-agent messaging)

```bash
acs inbox push --recipient manager --type ticket_completed --payload '{"ticket_id":"t-001"}' --sender w-0
acs inbox list --agent manager
```

## How Workers Operate

1. The **manager** loop runs on a configurable cycle (default: 15 seconds). It scans for pending tickets, assigns them to idle workers, and processes completion messages from worker inboxes.

2. Each **worker** receives a ticket assignment, then:
   - Creates a **git worktree** at `.acs/worktrees/<worker_id>` on a new branch `acs/<ticket_id>-<random>`.
   - Spawns a **Claude Code CLI** subprocess inside that worktree with a system prompt tailored to the ticket's domain (e.g., `backend-dev`, `frontend-dev`, `devops`).
   - Waits for the subprocess to complete (with a configurable timeout).
   - On success, notifies the manager. On timeout or crash, re-queues the ticket.

3. Workers run in parallel. Each operates in its own isolated worktree so there are no merge conflicts during execution.

## Configuration

ACS stores its configuration in `.acs/config.toml`. It is created automatically by `acs init`.

```toml
[project]
name = "my-project"
default_workers = 2

[manager]
cycle_seconds = 15           # How often the manager checks for pending tickets
worker_timeout_seconds = 300 # Max time a worker can spend on a ticket
worker_poll_seconds = 3      # How often workers poll their inbox

[agents]
tool_path = "acs"            # Path to the acs binary (used by workers)
claude_path = "claude"       # Path to the Claude Code CLI binary
```

### Persona mapping

Domain-to-persona mapping controls the system prompt each worker receives. Defaults:

| Domain     | Persona        |
|------------|----------------|
| frontend   | frontend-dev   |
| backend    | backend-dev    |
| devops     | devops         |
| qa         | qa             |
| infra      | devops         |
| core       | tech-lead      |
| general    | backend-dev    |

Override in `config.toml`:

```toml
[personas.mapping]
frontend = "frontend-dev"
backend = "backend-dev"
```

## Self-Development

ACS can develop itself. To use ACS on its own codebase:

```bash
# 1. Initialize (if not already done)
acs init --auto

# 2. Run workers against the ACS repo itself
acs run --workers 2

# 3. Monitor progress
acs status
acs log --follow
```

Workers will create feature branches, implement changes, and the manager will review them. After workers finish, inspect and merge the branches:

```bash
git branch -a | grep acs/
git log acs/<branch-name>
git merge acs/<branch-name>
```

### Development workflow (manual)

```bash
# Run tests
cargo test

# Build
cargo build

# Run with logging
RUST_LOG=debug cargo run -- status
```

### Release workflow

Tagging `v<version>` triggers `.github/workflows/release.yml`, which:

```bash
cargo test
cargo build --release
scripts/package-release.sh <target> target/release/acs dist <version>
```

Each release publishes:

- `acs-<version>-macos-arm64.tar.gz`
- `acs-<version>-macos-x64.tar.gz`
- `acs-<version>-linux-x64.tar.gz`
- `acs.rb` Homebrew formula

## Project Structure

```
.acs/
  config.toml    # Project configuration
  project.db     # SQLite database (tickets, agents, inbox, KB, events)
  logs/          # Worker log files
  worktrees/     # Git worktrees for active workers
src/
  main.rs        # CLI entrypoint
  lib.rs         # Library root
  cli/           # CLI command handlers (init, run, status, log, ticket, kb, inbox)
  config.rs      # Configuration loading and defaults
  db.rs          # SQLite database layer
  manager.rs     # Manager loop (ticket assignment, review)
  worker.rs      # Worker loop (ticket execution via Claude Code)
  spawner.rs     # Git worktree and Claude process management
  prompts.rs     # System prompt generation
  models.rs      # Data models
```

## License

See [LICENSE](LICENSE) for details.
