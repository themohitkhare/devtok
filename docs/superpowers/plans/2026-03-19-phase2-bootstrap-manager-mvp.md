# Phase 2: Bootstrap + Single Manager MVP — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable `synapse-os init <repo>` to bootstrap a project (analyze repo/spec → create tickets), and `synapse-os run` to start a manager + workers that autonomously execute tickets via Claude Code CLI.

**Architecture:** An MCP tool server (Python, FastMCP/stdio) exposes brain operations (tickets, knowledge base, communication) to Claude Code CLI instances. The orchestrator spawns Claude Code with `--append-system-prompt` and a `.mcp.json` pointing to our tool server. Each agent persona (bootstrap, manager, worker) has a tailored system prompt and tool set.

**Tech Stack:** Python 3.11+, `mcp` (FastMCP), Claude Code CLI (`claude -p`), Redis (from Phase 1), existing orchestrator components

**Dependencies:** All Phase 1 components (brain, registry, process_manager, cron_engine, feedback_watcher, daemon, spacetimedb_client)

---

## File Structure

```
orchestrator/
├── synapse_os/
│   ├── tools/                      # MCP tool server (spawned by Claude Code via stdio)
│   │   ├── __init__.py
│   │   ├── server.py               # FastMCP entry point — registers all tools, connects to Redis
│   │   ├── ticket_tools.py         # create_ticket, assign_ticket, update_ticket, update_ticket_status
│   │   ├── knowledge_tools.py      # read_knowledge_base, write_knowledge_base
│   │   └── communication_tools.py  # send_agent_message, post_status_card, request_human_approval
│   ├── prompts/
│   │   ├── __init__.py
│   │   ├── bootstrap.py            # Bootstrap agent system prompt generator
│   │   ├── manager.py              # Manager agent system prompt generator
│   │   └── worker.py               # Worker agent system prompt generator
│   ├── spawner.py                  # Spawns Claude Code CLI with MCP config + system prompt
│   ├── bootstrap.py                # Bootstrap flow: init project → spawn bootstrap agent → wait
│   ├── runner.py                   # Run flow: start manager + workers → monitor
│   └── cli.py                      # CLI entry points: init, run, status
├── mcp.template.json               # Template for .mcp.json (copied to target project)
└── tests/
    ├── test_ticket_tools.py
    ├── test_knowledge_tools.py
    ├── test_communication_tools.py
    ├── test_spawner.py
    ├── test_prompts.py
    ├── test_bootstrap.py
    ├── test_runner.py
    └── test_cli.py
```

---

### Task 1: MCP Tool Server Skeleton + Dependencies

**Files:**
- Modify: `orchestrator/pyproject.toml`
- Create: `orchestrator/synapse_os/tools/__init__.py`
- Create: `orchestrator/synapse_os/tools/server.py`
- Create: `orchestrator/mcp.template.json`

- [ ] **Step 1: Add MCP dependency to pyproject.toml**

Add `"mcp[cli]>=1.2.0"` to the `dependencies` list in `pyproject.toml`. Also add `"click>=8.0.0"` for the CLI.

- [ ] **Step 2: Install updated dependencies**

Run: `cd orchestrator && pip3 install -e ".[dev]" --quiet --break-system-packages`

- [ ] **Step 3: Create the MCP server skeleton**

```python
# synapse_os/tools/__init__.py
```

```python
# synapse_os/tools/server.py
"""MCP tool server for Synapse OS agents.

Spawned by Claude Code via stdio transport. Connects to Redis
and exposes brain operations as MCP tools.
"""
from __future__ import annotations

import os
import sys
import asyncio
import logging

import redis.asyncio as aioredis
from mcp.server.fastmcp import FastMCP

from synapse_os.brain import Brain

# All logging to stderr — stdout is reserved for JSON-RPC
logging.basicConfig(level=logging.INFO, stream=sys.stderr)
logger = logging.getLogger(__name__)

# Server instance — tools are registered via decorators in other modules
mcp = FastMCP("synapse-os-tools")

# Global state set during startup
_redis: aioredis.Redis | None = None
_brain: Brain | None = None
_agent_id: str = ""
_agent_role: str = ""  # "bootstrap", "manager", "worker"
_manager_id: str = ""


def get_brain() -> Brain:
    assert _brain is not None, "Brain not initialized"
    return _brain


def get_redis() -> aioredis.Redis:
    assert _redis is not None, "Redis not initialized"
    return _redis


def get_agent_id() -> str:
    return _agent_id


def get_agent_role() -> str:
    return _agent_role


def get_manager_id() -> str:
    return _manager_id


async def _init() -> None:
    global _redis, _brain, _agent_id, _agent_role, _manager_id
    redis_url = os.environ.get("SYNAPSE_REDIS_URL", "redis://localhost:6379/0")
    _redis = aioredis.from_url(redis_url, decode_responses=True)
    _brain = Brain(_redis)
    _agent_id = os.environ.get("SYNAPSE_AGENT_ID", "unknown")
    _agent_role = os.environ.get("SYNAPSE_AGENT_ROLE", "worker")
    _manager_id = os.environ.get("SYNAPSE_MANAGER_ID", "")
    logger.info("MCP server initialized: agent=%s role=%s", _agent_id, _agent_role)


def main() -> None:
    asyncio.get_event_loop().run_until_complete(_init())

    # Import tool modules to register their @mcp.tool() decorators
    import synapse_os.tools.ticket_tools  # noqa: F401
    import synapse_os.tools.knowledge_tools  # noqa: F401
    import synapse_os.tools.communication_tools  # noqa: F401

    mcp.run(transport="stdio")


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: Create MCP config template**

```json
{
  "mcpServers": {
    "synapse-os-tools": {
      "type": "stdio",
      "command": "python3",
      "args": ["-m", "synapse_os.tools.server"],
      "env": {
        "SYNAPSE_REDIS_URL": "${SYNAPSE_REDIS_URL}",
        "SYNAPSE_AGENT_ID": "${SYNAPSE_AGENT_ID}",
        "SYNAPSE_AGENT_ROLE": "${SYNAPSE_AGENT_ROLE}",
        "SYNAPSE_MANAGER_ID": "${SYNAPSE_MANAGER_ID}",
        "PYTHONPATH": "${SYNAPSE_PYTHONPATH}"
      }
    }
  }
}
```

- [ ] **Step 5: Commit**

```bash
git add orchestrator/pyproject.toml orchestrator/synapse_os/tools/ orchestrator/mcp.template.json
git commit -m "feat(phase2): MCP tool server skeleton with FastMCP + Redis"
```

---

### Task 2: Ticket Tools (MCP)

**Files:**
- Create: `orchestrator/synapse_os/tools/ticket_tools.py`
- Create: `orchestrator/tests/test_ticket_tools.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_ticket_tools.py
import pytest
import json
from synapse_os.brain import Brain
from synapse_os.tools import ticket_tools
from synapse_os.tools.server import _brain, _redis

pytestmark = pytest.mark.asyncio


@pytest.fixture(autouse=True)
async def setup_server_globals(redis):
    """Inject fakeredis into MCP server globals."""
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = Brain(redis)
    srv._agent_id = "test-agent"
    srv._agent_role = "manager"
    srv._manager_id = ""
    yield


async def test_create_ticket(redis):
    result = await ticket_tools._create_ticket(
        title="Build auth", description="Add login/signup", domain="backend", priority=1,
    )
    parsed = json.loads(result)
    assert parsed["status"] == "created"
    ticket_id = parsed["ticket_id"]
    # Verify in Redis
    brain = Brain(redis)
    data = await redis.hgetall(brain.key_ticket(ticket_id))
    assert data["title"] == "Build auth"
    assert data["status"] == "pending"


async def test_assign_ticket(redis):
    brain = Brain(redis)
    # Create ticket first
    await redis.hset(brain.key_ticket("t-1"), mapping={
        "ticket_id": "t-1", "title": "Test", "description": "Test",
        "domain": "backend", "priority": "1", "status": "pending", "assignee": "", "notes": "",
    })
    result = await ticket_tools._assign_ticket(ticket_id="t-1", agent_id="w-1")
    parsed = json.loads(result)
    assert parsed["status"] == "assigned"
    # Verify in worker inbox
    msg = await brain.pop_inbox("w-1", timeout=1)
    assert msg is not None
    assert "t-1" in msg


async def test_update_ticket_status(redis):
    brain = Brain(redis)
    await redis.hset(brain.key_ticket("t-1"), mapping={
        "ticket_id": "t-1", "title": "Test", "description": "Test",
        "domain": "backend", "priority": "1", "status": "pending", "assignee": "", "notes": "",
    })
    result = await ticket_tools._update_ticket_status(ticket_id="t-1", status="in_progress")
    parsed = json.loads(result)
    assert parsed["status"] == "updated"
    assert await redis.hget(brain.key_ticket("t-1"), "status") == "in_progress"


async def test_worker_cannot_use_create_ticket(redis):
    import synapse_os.tools.server as srv
    srv._agent_role = "worker"
    result = await ticket_tools._create_ticket(
        title="Nope", description="Nope", domain="backend", priority=1,
    )
    assert "not authorized" in result.lower()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_ticket_tools.py -v`
Expected: FAIL

- [ ] **Step 3: Implement ticket tools**

```python
# synapse_os/tools/ticket_tools.py
"""MCP tools for ticket management."""
from __future__ import annotations

import json
import uuid

from synapse_os.models import InboxMessage, Ticket, TicketStatus
from synapse_os.tools.server import get_agent_id, get_agent_role, get_brain, get_redis, mcp


def _require_role(*allowed: str) -> str | None:
    role = get_agent_role()
    if role not in allowed:
        return json.dumps({"error": f"Not authorized. Role '{role}' cannot use this tool. Allowed: {allowed}"})
    return None


@mcp.tool()
async def create_ticket(title: str, description: str, domain: str, priority: int, assignee: str = "") -> str:
    """Create a new ticket and add it to the work queue.

    Args:
        title: Short ticket title
        description: Detailed description of the work
        domain: Domain this ticket belongs to (frontend, backend, devops, etc.)
        priority: Priority level (1=highest, 5=lowest)
        assignee: Optional agent ID to directly assign to
    """
    return await _create_ticket(title, description, domain, priority, assignee)


async def _create_ticket(title: str, description: str, domain: str, priority: int, assignee: str = "") -> str:
    denied = _require_role("manager", "bootstrap")
    if denied:
        return denied

    brain = get_brain()
    redis = get_redis()
    ticket_id = f"t-{uuid.uuid4().hex[:8]}"

    ticket = Ticket(
        ticket_id=ticket_id, title=title, description=description,
        domain=domain, priority=priority, status=TicketStatus.PENDING,
        assignee=assignee or None,
    )
    await redis.hset(brain.key_ticket(ticket_id), mapping=ticket.to_dict())

    if assignee:
        # Push directly to assignee inbox
        msg = InboxMessage(
            msg_type="ticket_assignment",
            payload={"ticket_id": ticket_id, "title": title, "description": description},
            sender=get_agent_id(),
        )
        await brain.push_inbox(assignee, msg.to_json())
    else:
        # Add to shared work queue
        await brain.enqueue_ticket(ticket_id)

    return json.dumps({"status": "created", "ticket_id": ticket_id})


@mcp.tool()
async def assign_ticket(ticket_id: str, agent_id: str) -> str:
    """Assign a ticket directly to a specific agent.

    Args:
        ticket_id: The ticket ID to assign
        agent_id: The worker agent ID to assign to
    """
    return await _assign_ticket(ticket_id, agent_id)


async def _assign_ticket(ticket_id: str, agent_id: str) -> str:
    denied = _require_role("manager")
    if denied:
        return denied

    brain = get_brain()
    redis = get_redis()

    # Update ticket
    await redis.hset(brain.key_ticket(ticket_id), mapping={"assignee": agent_id, "status": "pending"})

    # Push to worker inbox
    title = await redis.hget(brain.key_ticket(ticket_id), "title") or ""
    desc = await redis.hget(brain.key_ticket(ticket_id), "description") or ""
    msg = InboxMessage(
        msg_type="ticket_assignment",
        payload={"ticket_id": ticket_id, "title": title, "description": desc},
        sender=get_agent_id(),
    )
    await brain.push_inbox(agent_id, msg.to_json())

    return json.dumps({"status": "assigned", "ticket_id": ticket_id, "agent_id": agent_id})


@mcp.tool()
async def update_ticket(ticket_id: str, status: str, notes: str = "") -> str:
    """Update a ticket's status and notes. Manager only.

    Args:
        ticket_id: The ticket ID
        status: New status (pending, in_progress, review_pending, blocked, completed)
        notes: Optional notes to add
    """
    return await _update_ticket(ticket_id, status, notes)


async def _update_ticket(ticket_id: str, status: str, notes: str = "") -> str:
    denied = _require_role("manager")
    if denied:
        return denied

    redis = get_redis()
    brain = get_brain()
    mapping: dict[str, str] = {"status": status}
    if notes:
        mapping["notes"] = notes
    await redis.hset(brain.key_ticket(ticket_id), mapping=mapping)
    return json.dumps({"status": "updated", "ticket_id": ticket_id, "new_status": status})


@mcp.tool()
async def update_ticket_status(ticket_id: str, status: str) -> str:
    """Update ticket status. Workers may only set: in_progress, review_pending, blocked.

    Args:
        ticket_id: The ticket ID
        status: New status
    """
    return await _update_ticket_status(ticket_id, status)


async def _update_ticket_status(ticket_id: str, status: str) -> str:
    role = get_agent_role()
    allowed_worker_statuses = {"in_progress", "review_pending", "blocked"}

    if role == "worker" and status not in allowed_worker_statuses:
        return json.dumps({"error": f"Workers can only set status to: {allowed_worker_statuses}"})

    redis = get_redis()
    brain = get_brain()
    await redis.hset(brain.key_ticket(ticket_id), "status", status)
    return json.dumps({"status": "updated", "ticket_id": ticket_id, "new_status": status})
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && python3 -m pytest tests/test_ticket_tools.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/tools/ticket_tools.py orchestrator/tests/test_ticket_tools.py
git commit -m "feat(phase2): MCP ticket tools — create, assign, update tickets"
```

---

### Task 3: Knowledge Base Tools (MCP)

**Files:**
- Create: `orchestrator/synapse_os/tools/knowledge_tools.py`
- Create: `orchestrator/tests/test_knowledge_tools.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_knowledge_tools.py
import pytest
import json
from synapse_os.brain import Brain
from synapse_os.tools import knowledge_tools

pytestmark = pytest.mark.asyncio


@pytest.fixture(autouse=True)
async def setup_server_globals(redis):
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = Brain(redis)
    srv._agent_id = "test-mgr"
    srv._agent_role = "manager"
    srv._manager_id = ""
    yield


async def test_write_and_read_knowledge(redis):
    result = await knowledge_tools._write_knowledge(
        domain="backend", key="api_schema", value='{"users": "/api/v1/users"}', expected_version=0,
    )
    parsed = json.loads(result)
    assert parsed["status"] == "written"
    assert parsed["new_version"] == 1

    result = await knowledge_tools._read_knowledge(domain="backend", key="api_schema")
    parsed = json.loads(result)
    assert parsed["value"] == '{"users": "/api/v1/users"}'
    assert parsed["version"] == 1


async def test_write_version_conflict(redis):
    await knowledge_tools._write_knowledge("backend", "schema", "v1", 0)
    result = await knowledge_tools._write_knowledge("backend", "schema", "v2", 0)
    parsed = json.loads(result)
    assert parsed["status"] == "conflict"


async def test_read_missing_key(redis):
    result = await knowledge_tools._read_knowledge("backend", "nonexistent")
    parsed = json.loads(result)
    assert parsed["value"] is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_knowledge_tools.py -v`

- [ ] **Step 3: Implement knowledge tools**

```python
# synapse_os/tools/knowledge_tools.py
"""MCP tools for the shared knowledge base."""
from __future__ import annotations

import json

from synapse_os.tools.server import get_brain, mcp


@mcp.tool()
async def read_knowledge_base(domain: str, key: str) -> str:
    """Read a value from the shared project knowledge base.

    Args:
        domain: Domain prefix (e.g. 'frontend', 'backend', 'contract')
        key: The key to read
    """
    return await _read_knowledge(domain, key)


async def _read_knowledge(domain: str, key: str) -> str:
    brain = get_brain()
    value, version = await brain.read_knowledge(domain, key)
    return json.dumps({"domain": domain, "key": key, "value": value, "version": version})


@mcp.tool()
async def write_knowledge_base(domain: str, key: str, value: str, expected_version: int = 0) -> str:
    """Write a value to the shared project knowledge base with optimistic locking.

    Args:
        domain: Domain prefix (e.g. 'frontend', 'backend', 'contract')
        key: The key to write
        value: The value to store (typically JSON string)
        expected_version: Expected current version (0 for new keys). Write fails if version doesn't match.
    """
    return await _write_knowledge(domain, key, value, expected_version)


async def _write_knowledge(domain: str, key: str, value: str, expected_version: int = 0) -> str:
    brain = get_brain()
    ok = await brain.write_knowledge(domain, key, value, expected_version)
    if ok:
        _, new_ver = await brain.read_knowledge(domain, key)
        return json.dumps({"status": "written", "new_version": new_ver})
    return json.dumps({"status": "conflict", "message": "Version mismatch. Re-read and retry."})
```

- [ ] **Step 4: Run tests**

Run: `cd orchestrator && python3 -m pytest tests/test_knowledge_tools.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/tools/knowledge_tools.py orchestrator/tests/test_knowledge_tools.py
git commit -m "feat(phase2): MCP knowledge base tools — read/write with optimistic locking"
```

---

### Task 4: Communication Tools (MCP)

**Files:**
- Create: `orchestrator/synapse_os/tools/communication_tools.py`
- Create: `orchestrator/tests/test_communication_tools.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_communication_tools.py
import pytest
import json
from synapse_os.brain import Brain
from synapse_os.tools import communication_tools

pytestmark = pytest.mark.asyncio


@pytest.fixture(autouse=True)
async def setup_server_globals(redis):
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = Brain(redis)
    srv._agent_id = "mgr-1"
    srv._agent_role = "manager"
    srv._manager_id = ""
    yield


async def test_send_agent_message(redis):
    result = await communication_tools._send_agent_message("w-1", "Please prioritize the auth ticket")
    parsed = json.loads(result)
    assert parsed["status"] == "sent"

    brain = Brain(redis)
    msg = await brain.pop_inbox("w-1", timeout=1)
    assert msg is not None
    assert "prioritize" in json.loads(msg)["payload"]["text"]


async def test_post_status_card(redis):
    # This just verifies the function runs without error (SpacetimeDB is mocked)
    import synapse_os.tools.server as srv
    from unittest.mock import AsyncMock
    srv._stdb_client = AsyncMock()
    result = await communication_tools._post_status_card("Build phase complete", priority=1)
    parsed = json.loads(result)
    assert parsed["status"] == "posted"


async def test_worker_sends_completion(redis):
    import synapse_os.tools.server as srv
    srv._agent_role = "worker"
    srv._manager_id = "mgr-1"
    result = await communication_tools._notify_manager(
        ticket_id="t-1", pr_url="https://github.com/org/repo/pull/1",
    )
    parsed = json.loads(result)
    assert parsed["status"] == "notified"

    brain = Brain(redis)
    msg = await brain.pop_manager_inbox("mgr-1", timeout=1)
    assert msg is not None
    data = json.loads(msg)
    assert data["msg_type"] == "completion"
    assert data["payload"]["pr_url"] == "https://github.com/org/repo/pull/1"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_communication_tools.py -v`

- [ ] **Step 3: Implement communication tools**

```python
# synapse_os/tools/communication_tools.py
"""MCP tools for inter-agent communication."""
from __future__ import annotations

import json

from synapse_os.models import InboxMessage
from synapse_os.tools.server import get_agent_id, get_agent_role, get_brain, get_manager_id, mcp


@mcp.tool()
async def send_agent_message(agent_id: str, message: str) -> str:
    """Send a message to another agent's inbox.

    Args:
        agent_id: Target agent ID
        message: Message text
    """
    return await _send_agent_message(agent_id, message)


async def _send_agent_message(agent_id: str, message: str) -> str:
    brain = get_brain()
    msg = InboxMessage(
        msg_type="direct_message",
        payload={"text": message},
        sender=get_agent_id(),
    )
    await brain.push_inbox(agent_id, msg.to_json())
    return json.dumps({"status": "sent", "to": agent_id})


@mcp.tool()
async def post_status_card(content: str, priority: int = 0) -> str:
    """Post a status update card to Synapse feed for human visibility.

    Args:
        content: Status update text
        priority: Priority (0=normal, 1=high)
    """
    return await _post_status_card(content, priority)


async def _post_status_card(content: str, priority: int = 0) -> str:
    from synapse_os.tools.server import _stdb_client
    if _stdb_client:
        try:
            await _stdb_client.post_action_card(
                agent_id=1, project_id=1, visual_type="StatusUpdate",
                content=content, task_summary=f"Status from {get_agent_id()}", priority=priority,
            )
        except Exception:
            pass  # Non-critical — don't block agent work
    return json.dumps({"status": "posted", "content": content[:50]})


@mcp.tool()
async def notify_manager(ticket_id: str, pr_url: str = "", message: str = "") -> str:
    """Notify your manager that work is complete or needs attention.

    Args:
        ticket_id: The ticket this is about
        pr_url: URL of the pull request (if work is complete)
        message: Optional message
    """
    return await _notify_manager(ticket_id, pr_url, message)


async def _notify_manager(ticket_id: str, pr_url: str = "", message: str = "") -> str:
    brain = get_brain()
    manager_id = get_manager_id()
    if not manager_id:
        return json.dumps({"error": "No manager_id configured"})

    msg = InboxMessage(
        msg_type="completion" if pr_url else "status_update",
        payload={"ticket_id": ticket_id, "pr_url": pr_url, "message": message},
        sender=get_agent_id(),
    )
    await brain.push_manager_inbox(manager_id, msg.to_json())
    return json.dumps({"status": "notified", "manager_id": manager_id})
```

- [ ] **Step 4: Run tests**

Run: `cd orchestrator && python3 -m pytest tests/test_communication_tools.py -v`
Expected: 3 passed

- [ ] **Step 5: Add `_stdb_client` global to server.py**

Add to `server.py` globals section:

```python
_stdb_client: SpacetimeDBClient | None = None
```

And in `_init()`:

```python
from synapse_os.spacetimedb_client import SpacetimeDBClient
from synapse_os.config import Config
stdb_url = os.environ.get("SYNAPSE_STDB_URL", "http://localhost:3000")
stdb_module = os.environ.get("SYNAPSE_STDB_MODULE", "synapse-backend-g9cee")
cfg = Config(spacetimedb_base_url=stdb_url, spacetimedb_module=stdb_module)
_stdb_client = SpacetimeDBClient(cfg)
```

- [ ] **Step 6: Commit**

```bash
git add orchestrator/synapse_os/tools/communication_tools.py orchestrator/tests/test_communication_tools.py orchestrator/synapse_os/tools/server.py
git commit -m "feat(phase2): MCP communication tools — messaging, status cards, manager notifications"
```

---

### Task 5: System Prompts

**Files:**
- Create: `orchestrator/synapse_os/prompts/__init__.py`
- Create: `orchestrator/synapse_os/prompts/bootstrap.py`
- Create: `orchestrator/synapse_os/prompts/manager.py`
- Create: `orchestrator/synapse_os/prompts/worker.py`
- Create: `orchestrator/tests/test_prompts.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_prompts.py
from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.prompts.manager import build_manager_prompt
from synapse_os.prompts.worker import build_worker_prompt


def test_bootstrap_prompt_contains_key_instructions():
    prompt = build_bootstrap_prompt(repo_path="/tmp/myrepo", spec_text="Build a todo app")
    assert "analyze" in prompt.lower()
    assert "create_ticket" in prompt
    assert "/tmp/myrepo" in prompt
    assert "todo app" in prompt


def test_bootstrap_prompt_without_spec():
    prompt = build_bootstrap_prompt(repo_path="/tmp/myrepo")
    assert "/tmp/myrepo" in prompt
    assert "create_ticket" in prompt


def test_manager_prompt_contains_key_instructions():
    prompt = build_manager_prompt(domain="frontend", project_summary="React todo app")
    assert "frontend" in prompt
    assert "assign_ticket" in prompt
    assert "create_ticket" in prompt


def test_worker_prompt_contains_key_instructions():
    prompt = build_worker_prompt(
        ticket_id="t-1", title="Build login page",
        description="Create a login form", domain="frontend",
    )
    assert "t-1" in prompt
    assert "Build login page" in prompt
    assert "notify_manager" in prompt
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_prompts.py -v`

- [ ] **Step 3: Implement prompts**

```python
# synapse_os/prompts/__init__.py
```

```python
# synapse_os/prompts/bootstrap.py
"""System prompt for the bootstrap agent."""


def build_bootstrap_prompt(repo_path: str, spec_text: str | None = None) -> str:
    spec_section = ""
    if spec_text:
        spec_section = f"""
## Project Spec

The human provided this spec/brief for the project:

{spec_text}

Use this to understand what needs to be built and break it down into tickets.
"""

    return f"""You are a Bootstrap Agent for Synapse OS — an autonomous project management system.

## Your Job

Analyze the repository at `{repo_path}` and create an initial set of tickets that represent the work needed.

{spec_section}

## Process

1. Read the repository structure (list files, read key files like README, package.json, Cargo.toml, etc.)
2. Understand the current state: what exists, what's working, what's missing
3. If a spec was provided, map spec requirements to implementation tasks
4. If no spec, identify improvements, bugs, missing tests, documentation gaps
5. Create tickets using the `create_ticket` tool. Each ticket should be:
   - Small enough for one agent to complete in one session
   - Self-contained with clear acceptance criteria in the description
   - Tagged with the right domain (frontend, backend, devops, etc.)
   - Prioritized (1=critical, 2=high, 3=medium, 4=low, 5=nice-to-have)
6. Write key architecture decisions to the knowledge base using `write_knowledge_base`
7. Post a summary status card using `post_status_card`

## Available Tools

- `create_ticket(title, description, domain, priority)` — create work tickets
- `write_knowledge_base(domain, key, value)` — store architecture decisions and project context
- `read_knowledge_base(domain, key)` — read stored knowledge
- `post_status_card(content, priority)` — post visible status updates

## Guidelines

- Create 5-15 tickets for a typical project. Don't over-decompose.
- Each ticket description should contain enough context for a developer who hasn't seen the repo
- Write domain-level summaries to the knowledge base (e.g. "frontend:stack" → "React 19, Vite, Tailwind")
- Prioritize tickets that unblock other work (foundations first, features second)
"""
```

```python
# synapse_os/prompts/manager.py
"""System prompt for the manager agent."""


def build_manager_prompt(domain: str, project_summary: str = "") -> str:
    return f"""You are a Manager Agent for Synapse OS — an autonomous project management system.

## Your Domain: {domain}

{f"Project context: {project_summary}" if project_summary else ""}

## Your Job

You manage a team of worker agents. Your responsibilities:
1. Review the work queue and assign tickets to available workers
2. Monitor worker progress via inbox messages
3. Create new tickets when you discover work that needs doing
4. Review completed work and close tickets
5. Escalate decisions to the human when needed

## How It Works

- Workers send you completion messages via `notify_manager`
- You read your inbox for status updates and completions
- You assign work using `assign_ticket` or create new work with `create_ticket`
- Post status updates for the human using `post_status_card`

## Available Tools

- `create_ticket(title, description, domain, priority, assignee?)` — create new tickets
- `assign_ticket(ticket_id, agent_id)` — push ticket to a specific worker
- `update_ticket(ticket_id, status, notes)` — update ticket status and notes
- `read_knowledge_base(domain, key)` — read project knowledge
- `write_knowledge_base(domain, key, value, expected_version)` — update project knowledge
- `send_agent_message(agent_id, message)` — send direct message to an agent
- `post_status_card(content, priority)` — post status update for human

## Decision Guidelines

- Assign tickets by domain expertise when possible
- If a worker is blocked, try to unblock them with context from the knowledge base
- If you can't resolve a blocker, escalate to the human via post_status_card with priority=1
- Close tickets when workers report completion with a PR URL
- Create follow-up tickets if a worker's completion reveals more work needed
"""
```

```python
# synapse_os/prompts/worker.py
"""System prompt for the worker agent."""


def build_worker_prompt(ticket_id: str, title: str, description: str, domain: str) -> str:
    return f"""You are a Worker Agent for Synapse OS — an autonomous project management system.

## Your Assignment

**Ticket:** {ticket_id}
**Title:** {title}
**Domain:** {domain}

**Description:**
{description}

## Your Job

1. Read the relevant code in the repository
2. Implement the changes described in the ticket
3. Write tests for your changes
4. Create a git branch, commit your work, and open a pull request
5. Notify your manager that the work is complete

## Process

1. Start by updating your ticket status: `update_ticket_status("{ticket_id}", "in_progress")`
2. Read the knowledge base for relevant context: `read_knowledge_base("{domain}", "stack")`, etc.
3. Do the implementation work (read files, write code, run tests)
4. Commit and push your changes to a feature branch
5. Open a PR with a clear title and description
6. Update ticket: `update_ticket_status("{ticket_id}", "review_pending")`
7. Notify manager: `notify_manager("{ticket_id}", pr_url="<the PR URL>")`

## Available Tools

- `update_ticket_status(ticket_id, status)` — update your ticket (in_progress, review_pending, blocked)
- `read_knowledge_base(domain, key)` — read project context and architecture decisions
- `notify_manager(ticket_id, pr_url, message)` — tell your manager you're done or need help
- `post_status_card(content, priority)` — post a visible status update

## Guidelines

- Stay focused on your ticket. Don't do extra work beyond what's described.
- If you're stuck, set status to "blocked" and notify your manager with a description of the blocker
- Always write tests for your changes
- Use clear, descriptive commit messages
- Open the PR against the main branch
"""
```

- [ ] **Step 4: Run tests**

Run: `cd orchestrator && python3 -m pytest tests/test_prompts.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/prompts/ orchestrator/tests/test_prompts.py
git commit -m "feat(phase2): system prompts for bootstrap, manager, and worker agents"
```

---

### Task 6: Agent Spawner

**Files:**
- Create: `orchestrator/synapse_os/spawner.py`
- Create: `orchestrator/tests/test_spawner.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_spawner.py
import json
import os
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.spawner import AgentSpawner
from synapse_os.config import Config

pytestmark = pytest.mark.asyncio


def test_build_mcp_config():
    spawner = AgentSpawner(config=Config(), project_dir="/tmp/myproject")
    config = spawner.build_mcp_config(
        agent_id="w-1", agent_role="worker", manager_id="m-1",
    )
    parsed = json.loads(config)
    server = parsed["mcpServers"]["synapse-os-tools"]
    assert server["type"] == "stdio"
    assert "synapse_os.tools.server" in " ".join(server["args"])
    assert server["env"]["SYNAPSE_AGENT_ID"] == "w-1"
    assert server["env"]["SYNAPSE_AGENT_ROLE"] == "worker"
    assert server["env"]["SYNAPSE_MANAGER_ID"] == "m-1"


def test_build_claude_command():
    spawner = AgentSpawner(config=Config(), project_dir="/tmp/myproject")
    cmd = spawner.build_claude_command(
        prompt="Do the work",
        system_prompt="You are a worker",
        allowed_tools=None,
    )
    assert "claude" in cmd[0]
    assert "-p" in cmd
    assert "--output-format" in cmd
    assert "json" in cmd
    assert "--append-system-prompt" in cmd


async def test_spawn_agent():
    spawner = AgentSpawner(config=Config(), project_dir="/tmp/myproject")

    with patch.object(spawner, '_write_mcp_config') as mock_write, \
         patch('synapse_os.spawner.ProcessManager') as MockPM:
        mock_pm_instance = AsyncMock()
        mock_proc = MagicMock()
        mock_proc.agent_id = "w-1"
        mock_proc.pid = 12345
        mock_proc.process = MagicMock()
        mock_proc.process.stdout = MagicMock()
        mock_pm_instance.spawn = AsyncMock(return_value=mock_proc)
        spawner._pm = mock_pm_instance

        result = await spawner.spawn_agent(
            agent_id="w-1",
            role="worker",
            prompt="Build the login page",
            system_prompt="You are a worker",
            manager_id="m-1",
        )
        assert result.agent_id == "w-1"
        mock_pm_instance.spawn.assert_called_once()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_spawner.py -v`

- [ ] **Step 3: Implement agent spawner**

```python
# synapse_os/spawner.py
"""Spawns Claude Code CLI instances with MCP tools and system prompts."""
from __future__ import annotations

import json
import os
import shutil
import tempfile

from synapse_os.config import Config
from synapse_os.process_manager import ManagedProcess, ProcessManager


class AgentSpawner:
    """Spawns Claude Code CLI as subprocesses with MCP tool configuration."""

    def __init__(self, config: Config, project_dir: str) -> None:
        self._config = config
        self._project_dir = project_dir
        self._pm = ProcessManager()
        self._orchestrator_pkg = os.path.dirname(os.path.abspath(__file__))

    def build_mcp_config(self, agent_id: str, agent_role: str, manager_id: str = "") -> str:
        """Build .mcp.json content for a specific agent."""
        # PYTHONPATH must include the orchestrator package root
        pkg_root = os.path.dirname(self._orchestrator_pkg)

        config = {
            "mcpServers": {
                "synapse-os-tools": {
                    "type": "stdio",
                    "command": "python3",
                    "args": ["-m", "synapse_os.tools.server"],
                    "env": {
                        "SYNAPSE_REDIS_URL": self._config.redis_url,
                        "SYNAPSE_AGENT_ID": agent_id,
                        "SYNAPSE_AGENT_ROLE": agent_role,
                        "SYNAPSE_MANAGER_ID": manager_id,
                        "SYNAPSE_STDB_URL": self._config.spacetimedb_base_url,
                        "SYNAPSE_STDB_MODULE": self._config.spacetimedb_module,
                        "PYTHONPATH": pkg_root,
                    },
                }
            }
        }
        return json.dumps(config, indent=2)

    def build_claude_command(
        self,
        prompt: str,
        system_prompt: str,
        allowed_tools: list[str] | None = None,
    ) -> list[str]:
        """Build the Claude Code CLI command."""
        cmd = [
            "claude", "-p", prompt,
            "--output-format", "json",
            "--append-system-prompt", system_prompt,
        ]
        if allowed_tools:
            cmd.extend(["--allowedTools", ",".join(allowed_tools)])
        return cmd

    def _write_mcp_config(self, agent_id: str, agent_role: str, manager_id: str) -> str:
        """Write .mcp.json to the project directory. Returns path."""
        config_content = self.build_mcp_config(agent_id, agent_role, manager_id)
        config_path = os.path.join(self._project_dir, ".mcp.json")
        with open(config_path, "w") as f:
            f.write(config_content)
        return config_path

    async def spawn_agent(
        self,
        agent_id: str,
        role: str,
        prompt: str,
        system_prompt: str,
        manager_id: str = "",
        allowed_tools: list[str] | None = None,
    ) -> ManagedProcess:
        """Spawn a Claude Code CLI instance with MCP tools configured."""
        self._write_mcp_config(agent_id, role, manager_id)
        cmd = self.build_claude_command(prompt, system_prompt, allowed_tools)
        return await self._pm.spawn(
            agent_id=agent_id,
            command=cmd,
            cwd=self._project_dir,
        )

    async def kill_agent(self, agent_id: str) -> bool:
        return await self._pm.kill(agent_id)

    async def kill_all(self) -> None:
        await self._pm.kill_all()
```

- [ ] **Step 4: Run tests**

Run: `cd orchestrator && python3 -m pytest tests/test_spawner.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/spawner.py orchestrator/tests/test_spawner.py
git commit -m "feat(phase2): agent spawner — launches Claude Code CLI with MCP tools"
```

---

### Task 7: Bootstrap Flow

**Files:**
- Create: `orchestrator/synapse_os/bootstrap.py`
- Create: `orchestrator/tests/test_bootstrap.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_bootstrap.py
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.bootstrap import BootstrapFlow
from synapse_os.config import Config

pytestmark = pytest.mark.asyncio


async def test_bootstrap_builds_correct_prompt():
    flow = BootstrapFlow(
        config=Config(), project_dir="/tmp/myrepo", spec_text="Build a todo app",
    )
    prompt = flow._build_prompt()
    assert "/tmp/myrepo" in prompt
    assert "todo app" in prompt


async def test_bootstrap_builds_prompt_without_spec():
    flow = BootstrapFlow(config=Config(), project_dir="/tmp/myrepo")
    prompt = flow._build_prompt()
    assert "/tmp/myrepo" in prompt


async def test_bootstrap_run_spawns_agent():
    flow = BootstrapFlow(
        config=Config(), project_dir="/tmp/myrepo", spec_text="Build a todo app",
    )

    mock_proc = MagicMock()
    mock_proc.agent_id = "bootstrap"
    mock_proc.pid = 999
    mock_proc.is_running = False  # Simulate already finished
    mock_proc.process = MagicMock()
    mock_proc.process.wait = AsyncMock(return_value=0)
    mock_proc.process.returncode = 0

    with patch.object(flow._spawner, 'spawn_agent', new_callable=AsyncMock, return_value=mock_proc):
        result = await flow.run()
        assert result["status"] == "complete"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_bootstrap.py -v`

- [ ] **Step 3: Implement bootstrap flow**

```python
# synapse_os/bootstrap.py
"""Bootstrap flow: analyze repo/spec → create tickets."""
from __future__ import annotations

import logging

from synapse_os.config import Config
from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.spawner import AgentSpawner

logger = logging.getLogger(__name__)


class BootstrapFlow:
    """Runs the bootstrap agent to analyze a repo and create initial tickets."""

    def __init__(self, config: Config, project_dir: str, spec_text: str | None = None) -> None:
        self._config = config
        self._project_dir = project_dir
        self._spec_text = spec_text
        self._spawner = AgentSpawner(config=config, project_dir=project_dir)

    def _build_prompt(self) -> str:
        system_prompt = build_bootstrap_prompt(
            repo_path=self._project_dir,
            spec_text=self._spec_text,
        )
        return system_prompt

    async def run(self) -> dict:
        """Run the bootstrap agent and wait for it to complete."""
        logger.info("Starting bootstrap for %s", self._project_dir)

        system_prompt = self._build_prompt()
        task_prompt = (
            f"Analyze the repository at {self._project_dir} and create tickets for all work needed. "
            "Use the create_ticket tool for each piece of work, and write_knowledge_base for key architecture info. "
            "Post a final status_card summarizing what you found."
        )

        proc = await self._spawner.spawn_agent(
            agent_id="bootstrap",
            role="bootstrap",
            prompt=task_prompt,
            system_prompt=system_prompt,
        )

        logger.info("Bootstrap agent spawned (pid=%d), waiting for completion...", proc.pid)
        returncode = await proc.process.wait()
        logger.info("Bootstrap agent finished (exit=%d)", returncode)

        await self._spawner.kill_all()

        return {
            "status": "complete" if returncode == 0 else "failed",
            "exit_code": returncode,
        }
```

- [ ] **Step 4: Run tests**

Run: `cd orchestrator && python3 -m pytest tests/test_bootstrap.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/bootstrap.py orchestrator/tests/test_bootstrap.py
git commit -m "feat(phase2): bootstrap flow — analyze repo and create initial tickets"
```

---

### Task 8: Runner (Manager + Workers Loop)

**Files:**
- Create: `orchestrator/synapse_os/runner.py`
- Create: `orchestrator/tests/test_runner.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_runner.py
import pytest
import json
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.models import TicketStatus
from synapse_os.runner import Runner

pytestmark = pytest.mark.asyncio


async def test_runner_init(redis):
    brain = Brain(redis)
    runner = Runner(
        config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain,
        domain="frontend", num_workers=2,
    )
    assert runner._domain == "frontend"
    assert runner._num_workers == 2


async def test_runner_creates_manager_and_workers(redis):
    brain = Brain(redis)
    runner = Runner(
        config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain,
        domain="frontend", num_workers=2,
    )

    mock_proc = MagicMock()
    mock_proc.agent_id = "test"
    mock_proc.pid = 100
    mock_proc.process = MagicMock()
    mock_proc.process.wait = AsyncMock(return_value=0)

    with patch.object(runner._spawner, 'spawn_agent', new_callable=AsyncMock, return_value=mock_proc):
        await runner.start()
        # Should have spawned 1 manager + 2 workers = 3 calls
        assert runner._spawner.spawn_agent.call_count == 3


async def test_runner_assigns_pending_tickets(redis):
    brain = Brain(redis)
    runner = Runner(
        config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain,
        domain="frontend", num_workers=1,
    )

    # Create a pending ticket
    await redis.hset(brain.key_ticket("t-1"), mapping={
        "ticket_id": "t-1", "title": "Test ticket", "description": "Do something",
        "domain": "frontend", "priority": "1", "status": "pending", "assignee": "", "notes": "",
    })
    await brain.enqueue_ticket("t-1")

    pending = await runner.get_pending_tickets()
    assert len(pending) >= 1
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_runner.py -v`

- [ ] **Step 3: Implement runner**

```python
# synapse_os/runner.py
"""Run flow: start manager + workers, monitor execution."""
from __future__ import annotations

import asyncio
import logging

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.models import AgentRole
from synapse_os.prompts.manager import build_manager_prompt
from synapse_os.prompts.worker import build_worker_prompt
from synapse_os.registry import AgentRegistry
from synapse_os.spawner import AgentSpawner

logger = logging.getLogger(__name__)


class Runner:
    """Manages a manager agent + N worker agents for a domain."""

    def __init__(
        self,
        config: Config,
        project_dir: str,
        redis: Redis,
        brain: Brain,
        domain: str = "general",
        num_workers: int = 2,
    ) -> None:
        self._config = config
        self._project_dir = project_dir
        self._redis = redis
        self._brain = brain
        self._domain = domain
        self._num_workers = num_workers
        self._spawner = AgentSpawner(config=config, project_dir=project_dir)
        self._registry = AgentRegistry(brain, redis)
        self._manager_id = f"mgr-{domain}"
        self._worker_ids: list[str] = []

    async def start(self) -> None:
        """Start the manager and worker agents."""
        logger.info("Starting runner for domain=%s with %d workers", self._domain, self._num_workers)

        # Spawn manager
        manager_prompt = build_manager_prompt(domain=self._domain)
        await self._spawner.spawn_agent(
            agent_id=self._manager_id,
            role="manager",
            prompt=f"You are the {self._domain} domain manager. Check your inbox for messages, review the work queue, and assign tickets to your workers.",
            system_prompt=manager_prompt,
        )
        await self._registry.register(
            self._manager_id, AgentRole.MANAGER, self._domain, "", 0,
        )
        logger.info("Manager %s started", self._manager_id)

        # Spawn workers
        for i in range(self._num_workers):
            worker_id = f"w-{self._domain}-{i}"
            self._worker_ids.append(worker_id)
            worker_prompt = build_worker_prompt(
                ticket_id="(will be assigned)", title="(awaiting assignment)",
                description="Wait for a ticket assignment from your manager.", domain=self._domain,
            )
            await self._spawner.spawn_agent(
                agent_id=worker_id,
                role="worker",
                prompt="Wait for ticket assignment. Check your inbox for a ticket_assignment message, then execute the described work.",
                system_prompt=worker_prompt,
                manager_id=self._manager_id,
            )
            await self._registry.register(
                worker_id, AgentRole.WORKER, self._domain, self._manager_id, 0,
            )
            logger.info("Worker %s started", worker_id)

    async def get_pending_tickets(self) -> list[dict]:
        """Get pending tickets from the work queue (non-blocking peek)."""
        tickets = []
        queue_len = await self._redis.llen(Brain.WORK_QUEUE_KEY)
        for i in range(queue_len):
            ticket_id = await self._redis.lindex(Brain.WORK_QUEUE_KEY, i)
            if ticket_id:
                data = await self._redis.hgetall(self._brain.key_ticket(ticket_id))
                if data:
                    tickets.append(data)
        return tickets

    async def stop(self) -> None:
        """Stop all agents."""
        await self._spawner.kill_all()
        for wid in self._worker_ids:
            await self._registry.deregister(wid)
        await self._registry.deregister(self._manager_id)
        logger.info("Runner stopped for domain=%s", self._domain)
```

- [ ] **Step 4: Run tests**

Run: `cd orchestrator && python3 -m pytest tests/test_runner.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/runner.py orchestrator/tests/test_runner.py
git commit -m "feat(phase2): runner — starts manager + workers, monitors ticket execution"
```

---

### Task 9: CLI Entry Point

**Files:**
- Create: `orchestrator/synapse_os/cli.py`
- Create: `orchestrator/tests/test_cli.py`
- Modify: `orchestrator/pyproject.toml` (update entry point)

- [ ] **Step 1: Write the failing test**

```python
# tests/test_cli.py
import pytest
from click.testing import CliRunner
from synapse_os.cli import cli


def test_cli_help():
    runner = CliRunner()
    result = runner.invoke(cli, ["--help"])
    assert result.exit_code == 0
    assert "init" in result.output
    assert "run" in result.output
    assert "status" in result.output


def test_init_requires_path():
    runner = CliRunner()
    result = runner.invoke(cli, ["init"])
    assert result.exit_code != 0  # Missing required argument


def test_run_requires_path():
    runner = CliRunner()
    result = runner.invoke(cli, ["run"])
    assert result.exit_code != 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && python3 -m pytest tests/test_cli.py -v`

- [ ] **Step 3: Implement CLI**

```python
# synapse_os/cli.py
"""CLI entry points for Synapse OS."""
from __future__ import annotations

import asyncio
import logging
import os

import click

from synapse_os.config import Config


@click.group()
def cli():
    """Synapse OS — Autonomous Project Management"""
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(name)s] %(levelname)s: %(message)s",
    )


@cli.command()
@click.argument("path", type=click.Path(exists=True))
@click.option("--spec", type=click.Path(exists=True), help="Path to spec/brief file")
def init(path: str, spec: str | None):
    """Bootstrap a project: analyze repo and create initial tickets."""
    from synapse_os.bootstrap import BootstrapFlow

    config = Config.from_env()
    abs_path = os.path.abspath(path)

    spec_text = None
    if spec:
        with open(spec) as f:
            spec_text = f.read()

    click.echo(f"Bootstrapping project at {abs_path}...")

    flow = BootstrapFlow(config=config, project_dir=abs_path, spec_text=spec_text)
    result = asyncio.run(flow.run())

    if result["status"] == "complete":
        click.echo("Bootstrap complete! Tickets created.")
    else:
        click.echo(f"Bootstrap failed (exit code {result['exit_code']})")
        raise SystemExit(1)


@cli.command()
@click.argument("path", type=click.Path(exists=True))
@click.option("--domain", default="general", help="Domain for this team")
@click.option("--workers", default=2, type=int, help="Number of worker agents")
def run(path: str, domain: str, workers: int):
    """Start manager + workers to execute tickets."""
    import redis.asyncio as aioredis

    from synapse_os.brain import Brain
    from synapse_os.runner import Runner

    config = Config.from_env()
    abs_path = os.path.abspath(path)

    async def _run():
        r = aioredis.from_url(config.redis_url, decode_responses=True)
        brain = Brain(r)
        runner = Runner(
            config=config, project_dir=abs_path, redis=r, brain=brain,
            domain=domain, num_workers=workers,
        )
        click.echo(f"Starting {domain} team with {workers} workers...")
        await runner.start()
        click.echo("Agents running. Press Ctrl+C to stop.")

        try:
            while True:
                await asyncio.sleep(1)
        except KeyboardInterrupt:
            pass
        finally:
            await runner.stop()
            await r.aclose()
            click.echo("Stopped.")

    asyncio.run(_run())


@cli.command()
def status():
    """Show current project status."""
    import redis.asyncio as aioredis

    from synapse_os.brain import Brain
    from synapse_os.registry import AgentRegistry

    config = Config.from_env()

    async def _status():
        r = aioredis.from_url(config.redis_url, decode_responses=True)
        brain = Brain(r)
        registry = AgentRegistry(brain, r)

        agents = await registry.list_agents()
        queue_len = await r.llen(Brain.WORK_QUEUE_KEY)

        click.echo(f"Registered agents: {len(agents)}")
        for a in agents:
            click.echo(f"  {a.agent_id} ({a.role.value}) — {a.status}")
        click.echo(f"Work queue: {queue_len} tickets pending")

        await r.aclose()

    asyncio.run(_status())


def main():
    cli()
```

- [ ] **Step 4: Update pyproject.toml entry point**

Change the scripts section:

```toml
[project.scripts]
synapse-os = "synapse_os.cli:main"
```

- [ ] **Step 5: Run tests**

Run: `cd orchestrator && pip3 install -e ".[dev]" --quiet --break-system-packages && python3 -m pytest tests/test_cli.py -v`
Expected: 3 passed

- [ ] **Step 6: Commit**

```bash
git add orchestrator/synapse_os/cli.py orchestrator/tests/test_cli.py orchestrator/pyproject.toml
git commit -m "feat(phase2): CLI — synapse-os init, run, status commands"
```

---

### Task 10: End-to-End Integration Test

**Files:**
- Create: `orchestrator/tests/test_e2e_phase2.py`

- [ ] **Step 1: Write the integration test**

```python
# tests/test_e2e_phase2.py
"""Phase 2 integration test: bootstrap creates tickets, runner can read them."""
import json
import pytest
from unittest.mock import AsyncMock

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.models import InboxMessage, Ticket, TicketStatus
from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.prompts.manager import build_manager_prompt
from synapse_os.prompts.worker import build_worker_prompt
from synapse_os.runner import Runner

pytestmark = pytest.mark.asyncio


async def test_bootstrap_to_runner_flow(redis):
    """Simulate: bootstrap creates tickets → runner picks them up."""
    brain = Brain(redis)
    config = Config()

    # 1. Simulate what the bootstrap agent would do (create tickets)
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = brain
    srv._agent_id = "bootstrap"
    srv._agent_role = "bootstrap"

    from synapse_os.tools.ticket_tools import _create_ticket
    result1 = await _create_ticket("Build auth", "Implement login/signup", "backend", 1)
    result2 = await _create_ticket("Add tests", "Write unit tests for auth", "backend", 2)

    t1 = json.loads(result1)
    t2 = json.loads(result2)
    assert t1["status"] == "created"
    assert t2["status"] == "created"

    # 2. Verify tickets are in the work queue
    runner = Runner(
        config=config, project_dir="/tmp/test", redis=redis, brain=brain,
        domain="backend", num_workers=1,
    )
    pending = await runner.get_pending_tickets()
    assert len(pending) == 2

    # 3. Simulate worker claiming a ticket
    ticket_id = await brain.claim_ticket(timeout=1)
    assert ticket_id is not None
    await brain.register_claim(ticket_id, "w-backend-0")

    # 4. Simulate worker completion → manager notification
    srv._agent_role = "worker"
    srv._manager_id = "mgr-backend"
    from synapse_os.tools.communication_tools import _notify_manager
    await _notify_manager(ticket_id, pr_url="https://github.com/test/pull/1")

    msg_raw = await brain.pop_manager_inbox("mgr-backend", timeout=1)
    msg = InboxMessage.from_json(msg_raw)
    assert msg.msg_type == "completion"
    assert msg.payload["pr_url"] == "https://github.com/test/pull/1"


async def test_prompts_are_well_formed():
    """Verify all prompt generators produce non-empty, useful prompts."""
    bp = build_bootstrap_prompt("/tmp/repo", "Build a todo app")
    assert len(bp) > 200
    assert "create_ticket" in bp

    mp = build_manager_prompt("frontend", "React app")
    assert len(mp) > 200
    assert "assign_ticket" in mp

    wp = build_worker_prompt("t-1", "Build login", "Create login form", "frontend")
    assert len(wp) > 200
    assert "notify_manager" in wp
```

- [ ] **Step 2: Run test**

Run: `cd orchestrator && python3 -m pytest tests/test_e2e_phase2.py -v`
Expected: 2 passed

- [ ] **Step 3: Run full test suite**

Run: `cd orchestrator && python3 -m pytest tests/ -v --tb=short`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add orchestrator/tests/test_e2e_phase2.py
git commit -m "test(phase2): end-to-end integration — bootstrap creates tickets, workers claim and complete"
```
