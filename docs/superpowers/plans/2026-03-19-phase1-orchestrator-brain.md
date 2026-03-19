# Phase 1: Process Orchestrator + Project Brain — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the foundational orchestration daemon and Redis-based project brain that manages CLI subprocess lifecycle, routes output to SpacetimeDB, and forwards human feedback from Synapse back to agent inboxes.

**Architecture:** A Python async daemon (`synapse_os`) manages Claude Code / Cursor CLI processes as subprocesses. Redis serves as the shared state layer (work queues, agent inboxes, heartbeats). The daemon bridges SpacetimeDB (existing Synapse) and Redis — routing CLI stdout into ActionCards and forwarding human feedback gestures into agent inboxes.

**Tech Stack:** Python 3.11+, asyncio, redis (async), httpx, pytest, pytest-asyncio, fakeredis

---

## File Structure

```
orchestrator/                          # New top-level directory (sibling to frontend/, worker/, backend/)
├── pyproject.toml                     # Package config, dependencies, scripts
├── synapse_os/
│   ├── __init__.py                    # Package init, version
│   ├── config.py                      # Dataclass config: Redis URL, SpacetimeDB URL, cron intervals
│   ├── models.py                      # Dataclasses: Ticket, AgentState, InboxMessage, StandupResponse
│   ├── brain.py                       # Redis client: connection, key helpers, Lua scripts (atomic claim, optimistic lock)
│   ├── registry.py                    # Agent registration, heartbeat updates, state queries
│   ├── spacetimedb_client.py          # HTTP client for SpacetimeDB reducers + SQL queries (reuses patterns from worker/main.py)
│   ├── process_manager.py             # Spawn/kill/restart asyncio subprocesses for CLI tools
│   ├── stdout_router.py               # Reads subprocess stdout lines → posts ActionCards via SpacetimeDB client
│   ├── health_checker.py              # Periodic heartbeat monitor, crash detection, restart triggers
│   ├── cron_engine.py                 # Cron tick scheduler: standup triggers, blocker detection
│   ├── feedback_watcher.py            # Polls SpacetimeDB feedback table → RPUSH to Redis agent inboxes
│   └── daemon.py                      # Main entry point: wires all components, signal handling, graceful shutdown
└── tests/
    ├── conftest.py                    # Shared fixtures: fakeredis instance, httpx mock transport
    ├── test_config.py                 # Config loading tests
    ├── test_models.py                 # Model serialization tests
    ├── test_brain.py                  # Redis client + Lua script tests
    ├── test_registry.py               # Agent registration + heartbeat tests
    ├── test_spacetimedb_client.py     # SpacetimeDB HTTP client tests (mocked HTTP)
    ├── test_process_manager.py        # Subprocess spawn/kill tests
    ├── test_stdout_router.py          # Stdout → ActionCard routing tests
    ├── test_health_checker.py         # Health check + restart trigger tests
    ├── test_cron_engine.py            # Cron scheduling tests
    └── test_feedback_watcher.py       # Feedback polling + Redis forwarding tests
```

---

### Task 1: Project Scaffolding + Config

**Files:**
- Create: `orchestrator/pyproject.toml`
- Create: `orchestrator/synapse_os/__init__.py`
- Create: `orchestrator/synapse_os/config.py`
- Create: `orchestrator/tests/conftest.py`
- Create: `orchestrator/tests/test_config.py`

- [ ] **Step 1: Create directory structure**

```bash
mkdir -p orchestrator/synapse_os orchestrator/tests
```

- [ ] **Step 2: Write pyproject.toml**

```toml
[project]
name = "synapse-os"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "redis>=5.0.0",
    "httpx>=0.27.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0.0",
    "pytest-asyncio>=0.24.0",
    "fakeredis>=2.21.0",
]

[project.scripts]
synapse-os = "synapse_os.daemon:main"
```

- [ ] **Step 3: Write `synapse_os/__init__.py`**

```python
__version__ = "0.1.0"
```

- [ ] **Step 4: Write the failing test for config**

```python
# tests/test_config.py
from synapse_os.config import Config


def test_default_config():
    cfg = Config()
    assert cfg.redis_url == "redis://localhost:6379/0"
    assert cfg.spacetimedb_base_url == "http://localhost:3000"
    assert cfg.spacetimedb_module == "synapse-backend-g9cee"
    assert cfg.standup_interval_seconds == 86400
    assert cfg.heartbeat_timeout_seconds == 60
    assert cfg.blocked_threshold_seconds == 1800
    assert cfg.feedback_poll_interval_seconds == 5


def test_config_from_env(monkeypatch):
    monkeypatch.setenv("SYNAPSE_REDIS_URL", "redis://custom:6380/1")
    monkeypatch.setenv("SYNAPSE_STANDUP_INTERVAL", "3600")
    cfg = Config.from_env()
    assert cfg.redis_url == "redis://custom:6380/1"
    assert cfg.standup_interval_seconds == 3600
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cd orchestrator && pip install -e ".[dev]" && pytest tests/test_config.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'synapse_os.config'`

- [ ] **Step 6: Write config implementation**

```python
# synapse_os/config.py
import os
from dataclasses import dataclass


@dataclass
class Config:
    redis_url: str = "redis://localhost:6379/0"
    spacetimedb_base_url: str = "http://localhost:3000"
    spacetimedb_module: str = "synapse-backend-g9cee"
    standup_interval_seconds: int = 86400
    heartbeat_timeout_seconds: int = 60
    blocked_threshold_seconds: int = 1800
    feedback_poll_interval_seconds: int = 5
    escalation_timeout_hours: int = 24

    @classmethod
    def from_env(cls) -> "Config":
        return cls(
            redis_url=os.getenv("SYNAPSE_REDIS_URL", cls.redis_url),
            spacetimedb_base_url=os.getenv("SYNAPSE_STDB_URL", cls.spacetimedb_base_url),
            spacetimedb_module=os.getenv("SYNAPSE_STDB_MODULE", cls.spacetimedb_module),
            standup_interval_seconds=int(os.getenv("SYNAPSE_STANDUP_INTERVAL", cls.standup_interval_seconds)),
            heartbeat_timeout_seconds=int(os.getenv("SYNAPSE_HEARTBEAT_TIMEOUT", cls.heartbeat_timeout_seconds)),
            blocked_threshold_seconds=int(os.getenv("SYNAPSE_BLOCKED_THRESHOLD", cls.blocked_threshold_seconds)),
            feedback_poll_interval_seconds=int(os.getenv("SYNAPSE_FEEDBACK_POLL", cls.feedback_poll_interval_seconds)),
            escalation_timeout_hours=int(os.getenv("SYNAPSE_ESCALATION_TIMEOUT", cls.escalation_timeout_hours)),
        )
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_config.py -v`
Expected: 2 passed

- [ ] **Step 8: Write conftest.py with shared fixtures**

```python
# tests/conftest.py
import pytest
import fakeredis.aioredis


@pytest.fixture
async def redis():
    """Provide a fresh fakeredis async client per test."""
    client = fakeredis.aioredis.FakeRedis(decode_responses=True)
    yield client
    await client.aclose()
```

- [ ] **Step 9: Commit**

```bash
git add orchestrator/
git commit -m "feat(phase1): project scaffolding, config, test fixtures"
```

---

### Task 2: Data Models

**Files:**
- Create: `orchestrator/synapse_os/models.py`
- Create: `orchestrator/tests/test_models.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_models.py
import json
from synapse_os.models import (
    AgentState,
    AgentRole,
    InboxMessage,
    Ticket,
    TicketStatus,
    StandupResponse,
)


def test_agent_state_to_dict():
    state = AgentState(
        agent_id="agent-1",
        role=AgentRole.WORKER,
        domain="frontend",
        status="idle",
        current_ticket=None,
        manager_id="mgr-1",
        process_pid=12345,
    )
    d = state.to_dict()
    assert d["agent_id"] == "agent-1"
    assert d["role"] == "worker"
    assert d["status"] == "idle"
    assert d["current_ticket"] == ""


def test_agent_state_from_dict():
    d = {
        "agent_id": "agent-1",
        "role": "worker",
        "domain": "frontend",
        "status": "idle",
        "current_ticket": "",
        "manager_id": "mgr-1",
        "process_pid": "12345",
        "last_heartbeat": "1000000",
    }
    state = AgentState.from_dict(d)
    assert state.agent_id == "agent-1"
    assert state.role == AgentRole.WORKER
    assert state.current_ticket is None
    assert state.process_pid == 12345


def test_ticket_to_dict():
    t = Ticket(
        ticket_id="t-1",
        title="Fix login",
        description="Auth is broken",
        domain="backend",
        priority=1,
        status=TicketStatus.PENDING,
    )
    d = t.to_dict()
    assert d["status"] == "pending"
    assert d["assignee"] == ""


def test_inbox_message_roundtrip():
    msg = InboxMessage(
        msg_type="ticket_assignment",
        payload={"ticket_id": "t-1"},
        sender="mgr-1",
    )
    serialized = msg.to_json()
    restored = InboxMessage.from_json(serialized)
    assert restored.msg_type == "ticket_assignment"
    assert restored.payload["ticket_id"] == "t-1"


def test_standup_response_to_json():
    sr = StandupResponse(
        agent_id="agent-1",
        did="Implemented auth endpoint",
        doing="Writing tests",
        blocked=None,
    )
    parsed = json.loads(sr.to_json())
    assert parsed["agent_id"] == "agent-1"
    assert parsed["blocked"] is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_models.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write models implementation**

```python
# synapse_os/models.py
from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class AgentRole(Enum):
    WORKER = "worker"
    MANAGER = "manager"
    BOOTSTRAP = "bootstrap"


class TicketStatus(Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    REVIEW_PENDING = "review_pending"
    BLOCKED = "blocked"
    COMPLETED = "completed"


@dataclass
class AgentState:
    agent_id: str
    role: AgentRole
    domain: str
    status: str  # idle, working, blocked, standup
    current_ticket: str | None
    manager_id: str
    process_pid: int
    last_heartbeat: int = 0

    def to_dict(self) -> dict[str, str]:
        return {
            "agent_id": self.agent_id,
            "role": self.role.value,
            "domain": self.domain,
            "status": self.status,
            "current_ticket": self.current_ticket or "",
            "manager_id": self.manager_id,
            "process_pid": str(self.process_pid),
            "last_heartbeat": str(self.last_heartbeat),
        }

    @classmethod
    def from_dict(cls, d: dict[str, str]) -> AgentState:
        return cls(
            agent_id=d["agent_id"],
            role=AgentRole(d["role"]),
            domain=d["domain"],
            status=d["status"],
            current_ticket=d["current_ticket"] or None,
            manager_id=d["manager_id"],
            process_pid=int(d["process_pid"]),
            last_heartbeat=int(d.get("last_heartbeat", "0")),
        )


@dataclass
class Ticket:
    ticket_id: str
    title: str
    description: str
    domain: str
    priority: int
    status: TicketStatus
    assignee: str | None = None
    notes: str = ""

    def to_dict(self) -> dict[str, str]:
        return {
            "ticket_id": self.ticket_id,
            "title": self.title,
            "description": self.description,
            "domain": self.domain,
            "priority": str(self.priority),
            "status": self.status.value,
            "assignee": self.assignee or "",
            "notes": self.notes,
        }

    @classmethod
    def from_dict(cls, d: dict[str, str]) -> Ticket:
        return cls(
            ticket_id=d["ticket_id"],
            title=d["title"],
            description=d["description"],
            domain=d["domain"],
            priority=int(d["priority"]),
            status=TicketStatus(d["status"]),
            assignee=d["assignee"] or None,
            notes=d.get("notes", ""),
        )


@dataclass
class InboxMessage:
    msg_type: str  # ticket_assignment, standup_request, standup_tick, meeting_invite, completion, escalation_response
    payload: dict[str, Any]
    sender: str
    timestamp: int = 0

    def to_json(self) -> str:
        return json.dumps({
            "msg_type": self.msg_type,
            "payload": self.payload,
            "sender": self.sender,
            "timestamp": self.timestamp,
        })

    @classmethod
    def from_json(cls, raw: str) -> InboxMessage:
        d = json.loads(raw)
        return cls(
            msg_type=d["msg_type"],
            payload=d["payload"],
            sender=d["sender"],
            timestamp=d.get("timestamp", 0),
        )


@dataclass
class StandupResponse:
    agent_id: str
    did: str
    doing: str
    blocked: str | None

    def to_json(self) -> str:
        return json.dumps({
            "agent_id": self.agent_id,
            "did": self.did,
            "doing": self.doing,
            "blocked": self.blocked,
        })

    @classmethod
    def from_json(cls, raw: str) -> StandupResponse:
        d = json.loads(raw)
        return cls(
            agent_id=d["agent_id"],
            did=d["did"],
            doing=d["doing"],
            blocked=d["blocked"],
        )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_models.py -v`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/models.py orchestrator/tests/test_models.py
git commit -m "feat(phase1): data models for agent state, tickets, inbox messages"
```

---

### Task 3: Redis Brain Client + Lua Scripts

**Files:**
- Create: `orchestrator/synapse_os/brain.py`
- Create: `orchestrator/tests/test_brain.py`

- [ ] **Step 1: Write the failing test for key helpers and basic ops**

```python
# tests/test_brain.py
import pytest
import json
from synapse_os.brain import Brain

pytestmark = pytest.mark.asyncio


async def test_key_helpers():
    """Brain produces correct Redis key names."""
    assert Brain.key_agent_inbox("a1") == "agent_inbox:a1"
    assert Brain.key_manager_inbox("m1") == "manager_inbox:m1"
    assert Brain.key_agent_state("a1") == "agent_state:a1"
    assert Brain.key_ticket("t1") == "tickets:t1"
    assert Brain.key_standup_responses("m1") == "standup_responses:m1"
    assert Brain.key_knowledge("frontend", "api") == "knowledge_base:frontend:api"
    assert Brain.key_knowledge_ver("api") == "knowledge_base_ver:api"
    assert Brain.key_meeting("meet-1") == "meeting:meet-1"


async def test_push_and_pop_inbox(redis):
    brain = Brain(redis)
    await brain.push_inbox("a1", '{"msg_type":"test","payload":{},"sender":"s"}')
    msg = await brain.pop_inbox("a1", timeout=1)
    assert msg is not None
    assert json.loads(msg)["msg_type"] == "test"


async def test_pop_inbox_empty_returns_none(redis):
    brain = Brain(redis)
    msg = await brain.pop_inbox("a1", timeout=1)
    assert msg is None


async def test_push_and_pop_manager_inbox(redis):
    brain = Brain(redis)
    await brain.push_manager_inbox("m1", '{"msg_type":"completion","payload":{},"sender":"w1"}')
    msg = await brain.pop_manager_inbox("m1", timeout=1)
    assert json.loads(msg)["msg_type"] == "completion"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_brain.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write Brain class with key helpers and inbox ops**

```python
# synapse_os/brain.py
from __future__ import annotations

from redis.asyncio import Redis


class Brain:
    """Redis-backed project brain. Thin wrapper over async Redis with key conventions."""

    def __init__(self, redis: Redis) -> None:
        self._r = redis

    # --- Key helpers ---

    @staticmethod
    def key_agent_inbox(agent_id: str) -> str:
        return f"agent_inbox:{agent_id}"

    @staticmethod
    def key_manager_inbox(manager_id: str) -> str:
        return f"manager_inbox:{manager_id}"

    @staticmethod
    def key_agent_state(agent_id: str) -> str:
        return f"agent_state:{agent_id}"

    @staticmethod
    def key_ticket(ticket_id: str) -> str:
        return f"tickets:{ticket_id}"

    @staticmethod
    def key_standup_responses(manager_id: str) -> str:
        return f"standup_responses:{manager_id}"

    @staticmethod
    def key_knowledge(domain: str, key: str) -> str:
        return f"knowledge_base:{domain}:{key}"

    @staticmethod
    def key_knowledge_ver(key: str) -> str:
        return f"knowledge_base_ver:{key}"

    @staticmethod
    def key_meeting(meeting_id: str) -> str:
        return f"meeting:{meeting_id}"

    @staticmethod
    def key_escalation_card(card_id: str) -> str:
        return f"escalation_card:{card_id}"

    # --- Inbox operations ---

    async def push_inbox(self, agent_id: str, message_json: str) -> None:
        await self._r.rpush(self.key_agent_inbox(agent_id), message_json)

    async def pop_inbox(self, agent_id: str, timeout: int = 0) -> str | None:
        result = await self._r.blpop(self.key_agent_inbox(agent_id), timeout=timeout)
        return result[1] if result else None

    async def push_manager_inbox(self, manager_id: str, message_json: str) -> None:
        await self._r.rpush(self.key_manager_inbox(manager_id), message_json)

    async def pop_manager_inbox(self, manager_id: str, timeout: int = 0) -> str | None:
        result = await self._r.blpop(self.key_manager_inbox(manager_id), timeout=timeout)
        return result[1] if result else None
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_brain.py -v`
Expected: 4 passed

- [ ] **Step 5: Write failing tests for work queue + atomic claim**

Add to `tests/test_brain.py`:

```python
async def test_enqueue_and_claim_ticket(redis):
    brain = Brain(redis)
    # Enqueue a ticket
    await brain.enqueue_ticket("t-1")
    # Claim it
    ticket_id = await brain.claim_ticket(timeout=1)
    assert ticket_id == "t-1"
    # Verify in_progress was set
    owner = await redis.hget("in_progress", "t-1")
    assert owner is None  # claim_ticket doesn't set owner — caller does via register_claim


async def test_register_claim(redis):
    brain = Brain(redis)
    await brain.enqueue_ticket("t-1")
    # Set up a ticket hash first
    await redis.hset(brain.key_ticket("t-1"), mapping={"status": "pending", "assignee": ""})
    ticket_id = await brain.claim_ticket(timeout=1)
    result = await brain.register_claim(ticket_id, "agent-1")
    assert result is True
    # Verify atomically set
    assert await redis.hget("in_progress", "t-1") == "agent-1"
    assert await redis.hget(brain.key_ticket("t-1"), "status") == "in_progress"
    assert await redis.hget(brain.key_ticket("t-1"), "assignee") == "agent-1"


async def test_claim_empty_queue_returns_none(redis):
    brain = Brain(redis)
    ticket_id = await brain.claim_ticket(timeout=1)
    assert ticket_id is None
```

- [ ] **Step 6: Run tests to verify new ones fail**

Run: `cd orchestrator && pytest tests/test_brain.py -v`
Expected: 3 new FAIL — `AttributeError: 'Brain' object has no attribute 'enqueue_ticket'`

- [ ] **Step 7: Implement work queue + Lua-based atomic claim**

Add to `synapse_os/brain.py`:

```python
    # --- Work queue ---

    WORK_QUEUE_KEY = "work_queue"

    # Lua script: atomically set in_progress, ticket status, and assignee
    _CLAIM_LUA = """
    local ticket_key = KEYS[1]
    local ticket_id = ARGV[1]
    local agent_id = ARGV[2]
    redis.call('HSET', 'in_progress', ticket_id, agent_id)
    redis.call('HSET', ticket_key, 'status', 'in_progress')
    redis.call('HSET', ticket_key, 'assignee', agent_id)
    return 1
    """

    async def enqueue_ticket(self, ticket_id: str) -> None:
        await self._r.rpush(self.WORK_QUEUE_KEY, ticket_id)

    async def claim_ticket(self, timeout: int = 0) -> str | None:
        result = await self._r.blpop(self.WORK_QUEUE_KEY, timeout=timeout)
        return result[1] if result else None

    async def register_claim(self, ticket_id: str, agent_id: str) -> bool:
        result = await self._r.eval(
            self._CLAIM_LUA,
            1,
            self.key_ticket(ticket_id),
            ticket_id,
            agent_id,
        )
        return result == 1
```

- [ ] **Step 8: Run all brain tests**

Run: `cd orchestrator && pytest tests/test_brain.py -v`
Expected: 7 passed

- [ ] **Step 9: Write failing tests for knowledge base with optimistic locking**

Add to `tests/test_brain.py`:

```python
async def test_knowledge_write_new_key(redis):
    brain = Brain(redis)
    ok = await brain.write_knowledge("frontend", "api_shape", '{"endpoint": "/users"}', expected_version=0)
    assert ok is True
    val = await redis.get(brain.key_knowledge("frontend", "api_shape"))
    assert val == '{"endpoint": "/users"}'
    ver = await redis.get(brain.key_knowledge_ver("api_shape"))
    assert ver == "1"


async def test_knowledge_write_version_conflict(redis):
    brain = Brain(redis)
    await brain.write_knowledge("frontend", "api_shape", "v1", expected_version=0)
    # Try to write with stale version
    ok = await brain.write_knowledge("frontend", "api_shape", "v2", expected_version=0)
    assert ok is False  # Version is now 1, not 0


async def test_knowledge_read(redis):
    brain = Brain(redis)
    await brain.write_knowledge("backend", "schema", "users_table", expected_version=0)
    value, version = await brain.read_knowledge("backend", "schema")
    assert value == "users_table"
    assert version == 1
```

- [ ] **Step 10: Run tests to verify new ones fail**

Run: `cd orchestrator && pytest tests/test_brain.py::test_knowledge_write_new_key -v`
Expected: FAIL — `AttributeError`

- [ ] **Step 11: Implement knowledge base with optimistic locking Lua script**

Add to `synapse_os/brain.py`:

```python
    # --- Knowledge base (optimistic locking) ---

    _KNOWLEDGE_WRITE_LUA = """
    local kb_key = KEYS[1]
    local ver_key = KEYS[2]
    local value = ARGV[1]
    local expected_ver = tonumber(ARGV[2])
    local current_ver = tonumber(redis.call('GET', ver_key) or '0')
    if current_ver ~= expected_ver then
        return 0
    end
    redis.call('SET', kb_key, value)
    redis.call('SET', ver_key, tostring(current_ver + 1))
    return 1
    """

    async def write_knowledge(self, domain: str, key: str, value: str, expected_version: int) -> bool:
        result = await self._r.eval(
            self._KNOWLEDGE_WRITE_LUA,
            2,
            self.key_knowledge(domain, key),
            self.key_knowledge_ver(key),
            value,
            str(expected_version),
        )
        return result == 1

    async def read_knowledge(self, domain: str, key: str) -> tuple[str | None, int]:
        value = await self._r.get(self.key_knowledge(domain, key))
        ver = await self._r.get(self.key_knowledge_ver(key))
        return value, int(ver) if ver else 0
```

- [ ] **Step 12: Run all brain tests**

Run: `cd orchestrator && pytest tests/test_brain.py -v`
Expected: 10 passed

- [ ] **Step 13: Commit**

```bash
git add orchestrator/synapse_os/brain.py orchestrator/tests/test_brain.py
git commit -m "feat(phase1): Redis brain client with inbox ops, work queue, knowledge base"
```

---

### Task 4: Agent Registry + Heartbeat

**Files:**
- Create: `orchestrator/synapse_os/registry.py`
- Create: `orchestrator/tests/test_registry.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_registry.py
import time
import pytest
from synapse_os.brain import Brain
from synapse_os.models import AgentRole
from synapse_os.registry import AgentRegistry

pytestmark = pytest.mark.asyncio


async def test_register_agent(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    await registry.register(
        agent_id="w-1",
        role=AgentRole.WORKER,
        domain="frontend",
        manager_id="m-1",
        process_pid=9999,
    )
    state = await registry.get_state("w-1")
    assert state is not None
    assert state.agent_id == "w-1"
    assert state.role == AgentRole.WORKER
    assert state.status == "idle"
    assert state.process_pid == 9999


async def test_heartbeat_updates_timestamp(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 9999)
    await registry.heartbeat("w-1")
    state = await registry.get_state("w-1")
    assert state.last_heartbeat > 0


async def test_update_status(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 9999)
    await registry.update_status("w-1", "working", current_ticket="t-1")
    state = await registry.get_state("w-1")
    assert state.status == "working"
    assert state.current_ticket == "t-1"


async def test_deregister(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 9999)
    await registry.deregister("w-1")
    state = await registry.get_state("w-1")
    assert state is None


async def test_list_agents_by_role(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 100)
    await registry.register("w-2", AgentRole.WORKER, "backend", "m-1", 101)
    await registry.register("m-1", AgentRole.MANAGER, "frontend", "", 200)
    workers = await registry.list_agents(role=AgentRole.WORKER)
    assert len(workers) == 2
    managers = await registry.list_agents(role=AgentRole.MANAGER)
    assert len(managers) == 1


async def test_get_stale_agents(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 100)
    # Set heartbeat to a time well in the past
    await redis.hset(brain.key_agent_state("w-1"), "last_heartbeat", "1000")
    stale = await registry.get_stale_agents(timeout_seconds=60)
    assert "w-1" in [s.agent_id for s in stale]
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_registry.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write registry implementation**

```python
# synapse_os/registry.py
from __future__ import annotations

import time

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.models import AgentRole, AgentState


class AgentRegistry:
    """Manages agent registration, heartbeats, and state in Redis."""

    # Redis set tracking all registered agent IDs
    _AGENTS_SET_KEY = "registered_agents"

    def __init__(self, brain: Brain, redis: Redis) -> None:
        self._brain = brain
        self._r = redis

    async def register(
        self,
        agent_id: str,
        role: AgentRole,
        domain: str,
        manager_id: str,
        process_pid: int,
    ) -> None:
        state = AgentState(
            agent_id=agent_id,
            role=role,
            domain=domain,
            status="idle",
            current_ticket=None,
            manager_id=manager_id,
            process_pid=process_pid,
            last_heartbeat=int(time.time()),
        )
        await self._r.hset(self._brain.key_agent_state(agent_id), mapping=state.to_dict())
        await self._r.sadd(self._AGENTS_SET_KEY, agent_id)

    async def deregister(self, agent_id: str) -> None:
        await self._r.delete(self._brain.key_agent_state(agent_id))
        await self._r.srem(self._AGENTS_SET_KEY, agent_id)

    async def heartbeat(self, agent_id: str) -> None:
        await self._r.hset(
            self._brain.key_agent_state(agent_id),
            "last_heartbeat",
            str(int(time.time())),
        )

    async def update_status(
        self,
        agent_id: str,
        status: str,
        current_ticket: str | None = None,
    ) -> None:
        mapping: dict[str, str] = {"status": status}
        if current_ticket is not None:
            mapping["current_ticket"] = current_ticket
        await self._r.hset(self._brain.key_agent_state(agent_id), mapping=mapping)

    async def get_state(self, agent_id: str) -> AgentState | None:
        data = await self._r.hgetall(self._brain.key_agent_state(agent_id))
        if not data:
            return None
        return AgentState.from_dict(data)

    async def list_agents(self, role: AgentRole | None = None) -> list[AgentState]:
        agent_ids = await self._r.smembers(self._AGENTS_SET_KEY)
        agents = []
        for aid in agent_ids:
            state = await self.get_state(aid)
            if state and (role is None or state.role == role):
                agents.append(state)
        return agents

    async def get_stale_agents(self, timeout_seconds: int) -> list[AgentState]:
        cutoff = int(time.time()) - timeout_seconds
        agents = await self.list_agents()
        return [a for a in agents if a.last_heartbeat < cutoff]
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_registry.py -v`
Expected: 6 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/registry.py orchestrator/tests/test_registry.py
git commit -m "feat(phase1): agent registry with registration, heartbeat, stale detection"
```

---

### Task 5: SpacetimeDB HTTP Client

**Files:**
- Create: `orchestrator/synapse_os/spacetimedb_client.py`
- Create: `orchestrator/tests/test_spacetimedb_client.py`

- [ ] **Step 1: Write the failing test with mocked HTTP**

```python
# tests/test_spacetimedb_client.py
import json
import pytest
import httpx
from synapse_os.config import Config
from synapse_os.spacetimedb_client import SpacetimeDBClient

pytestmark = pytest.mark.asyncio


class MockTransport(httpx.AsyncBaseTransport):
    """Records requests and returns canned responses."""

    def __init__(self):
        self.requests: list[httpx.Request] = []
        self.responses: list[httpx.Response] = []

    def add_response(self, status_code=200, json_data=None, text=None):
        if json_data is not None:
            self.responses.append(httpx.Response(status_code, json=json_data))
        else:
            self.responses.append(httpx.Response(status_code, text=text or ""))

    async def handle_async_request(self, request):
        self.requests.append(request)
        return self.responses.pop(0) if self.responses else httpx.Response(200)


@pytest.fixture
def mock_transport():
    return MockTransport()


@pytest.fixture
def stdb_client(mock_transport):
    cfg = Config()
    client = SpacetimeDBClient(cfg, transport=mock_transport)
    return client


async def test_call_reducer(stdb_client, mock_transport):
    mock_transport.add_response(status_code=200, text="")
    await stdb_client.call_reducer("insert_action_card", ["agent-1", "proj-1", "StatusUpdate", "content", "summary", 1])
    req = mock_transport.requests[0]
    assert "/call/insert_action_card" in str(req.url)
    body = json.loads(req.content)
    assert body[0] == "agent-1"


async def test_query_sql(stdb_client, mock_transport):
    mock_transport.add_response(
        json_data=[{"rows": [["row1-col1", "row1-col2"]]}]
    )
    rows = await stdb_client.query_sql("SELECT * FROM feedback")
    assert len(rows) == 1
    assert rows[0][0] == "row1-col1"


async def test_post_action_card(stdb_client, mock_transport):
    mock_transport.add_response(status_code=200, text="")
    await stdb_client.post_action_card(
        agent_id=1,
        project_id=1,
        visual_type="StatusUpdate",
        content="test content",
        task_summary="test summary",
        priority=1,
    )
    req = mock_transport.requests[0]
    assert "/call/insert_action_card" in str(req.url)


async def test_query_feedback_since(stdb_client, mock_transport):
    mock_transport.add_response(
        json_data=[{"rows": [
            [1, 10, "approve", "", 1000000],
            [2, 11, "reject", "bad code", 1000001],
        ]}]
    )
    rows = await stdb_client.query_feedback_since(999999)
    assert len(rows) == 2
    assert rows[0][2] == "approve"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_spacetimedb_client.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write SpacetimeDB client implementation**

```python
# synapse_os/spacetimedb_client.py
from __future__ import annotations

from typing import Any

import httpx

from synapse_os.config import Config


class SpacetimeDBClient:
    """HTTP client for SpacetimeDB reducers and SQL queries."""

    def __init__(self, config: Config, transport: httpx.AsyncBaseTransport | None = None) -> None:
        self._base = f"{config.spacetimedb_base_url}/v1/database/{config.spacetimedb_module}"
        kwargs: dict[str, Any] = {"timeout": 10.0}
        if transport:
            kwargs["transport"] = transport
        self._http = httpx.AsyncClient(**kwargs)

    async def call_reducer(self, reducer: str, args: list[Any]) -> None:
        url = f"{self._base}/call/{reducer}"
        resp = await self._http.post(url, json=args, headers={"Content-Type": "application/json"})
        resp.raise_for_status()

    async def query_sql(self, sql: str) -> list[list[Any]]:
        url = f"{self._base}/sql"
        resp = await self._http.post(url, content=sql, headers={"Content-Type": "text/plain"})
        resp.raise_for_status()
        chunks = resp.json()
        rows: list[list[Any]] = []
        for chunk in chunks:
            rows.extend(chunk.get("rows", []))
        return rows

    async def post_action_card(
        self,
        agent_id: int,
        project_id: int,
        visual_type: str,
        content: str,
        task_summary: str,
        priority: int,
    ) -> None:
        await self.call_reducer(
            "insert_action_card",
            [agent_id, project_id, visual_type, content, task_summary, priority],
        )

    async def query_feedback_since(self, since_micros: int) -> list[list[Any]]:
        sql = f"SELECT id, card_id, action_type, payload, created_at FROM feedback WHERE created_at > {since_micros}"
        return await self.query_sql(sql)

    async def close(self) -> None:
        await self._http.aclose()
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_spacetimedb_client.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/spacetimedb_client.py orchestrator/tests/test_spacetimedb_client.py
git commit -m "feat(phase1): SpacetimeDB HTTP client for reducers and SQL queries"
```

---

### Task 6: Process Manager (Spawn/Kill CLI Subprocesses)

**Files:**
- Create: `orchestrator/synapse_os/process_manager.py`
- Create: `orchestrator/tests/test_process_manager.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_process_manager.py
import asyncio
import pytest
from synapse_os.process_manager import ProcessManager, ManagedProcess

pytestmark = pytest.mark.asyncio


async def test_spawn_process():
    pm = ProcessManager()
    proc = await pm.spawn(
        agent_id="test-1",
        command=["python3", "-c", "import time; time.sleep(30)"],
        cwd="/tmp",
    )
    assert proc.agent_id == "test-1"
    assert proc.process.returncode is None  # still running
    await pm.kill("test-1")


async def test_kill_process():
    pm = ProcessManager()
    await pm.spawn("test-1", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    success = await pm.kill("test-1")
    assert success is True
    assert "test-1" not in pm.processes


async def test_kill_nonexistent():
    pm = ProcessManager()
    success = await pm.kill("no-such-agent")
    assert success is False


async def test_list_processes():
    pm = ProcessManager()
    await pm.spawn("a1", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    await pm.spawn("a2", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    procs = pm.list_running()
    assert len(procs) == 2
    await pm.kill_all()


async def test_detect_crashed():
    pm = ProcessManager()
    # Spawn a process that exits immediately
    await pm.spawn("crash-1", ["python3", "-c", "exit(1)"], "/tmp")
    await asyncio.sleep(0.2)  # Let it exit
    crashed = pm.get_crashed()
    assert "crash-1" in [p.agent_id for p in crashed]
    await pm.kill_all()


async def test_get_pid():
    pm = ProcessManager()
    proc = await pm.spawn("test-1", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    assert proc.pid > 0
    await pm.kill_all()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_process_manager.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write process manager implementation**

```python
# synapse_os/process_manager.py
from __future__ import annotations

import asyncio
import signal
from dataclasses import dataclass, field


@dataclass
class ManagedProcess:
    agent_id: str
    command: list[str]
    process: asyncio.subprocess.Process
    cwd: str

    @property
    def pid(self) -> int:
        return self.process.pid

    @property
    def is_running(self) -> bool:
        return self.process.returncode is None


class ProcessManager:
    """Manages CLI subprocesses for AI agents."""

    def __init__(self) -> None:
        self.processes: dict[str, ManagedProcess] = {}

    async def spawn(
        self,
        agent_id: str,
        command: list[str],
        cwd: str,
    ) -> ManagedProcess:
        proc = await asyncio.create_subprocess_exec(
            *command,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=cwd,
        )
        managed = ManagedProcess(
            agent_id=agent_id,
            command=command,
            process=proc,
            cwd=cwd,
        )
        self.processes[agent_id] = managed
        return managed

    async def kill(self, agent_id: str) -> bool:
        managed = self.processes.pop(agent_id, None)
        if not managed:
            return False
        if managed.is_running:
            managed.process.terminate()
            try:
                await asyncio.wait_for(managed.process.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                managed.process.kill()
                await managed.process.wait()
        return True

    async def kill_all(self) -> None:
        agent_ids = list(self.processes.keys())
        for aid in agent_ids:
            await self.kill(aid)

    def list_running(self) -> list[ManagedProcess]:
        return [p for p in self.processes.values() if p.is_running]

    def get_crashed(self) -> list[ManagedProcess]:
        return [p for p in self.processes.values() if not p.is_running]
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_process_manager.py -v`
Expected: 6 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/process_manager.py orchestrator/tests/test_process_manager.py
git commit -m "feat(phase1): process manager for spawning/killing CLI subprocesses"
```

---

### Task 7: Stdout Router (CLI Output → SpacetimeDB ActionCards)

**Files:**
- Create: `orchestrator/synapse_os/stdout_router.py`
- Create: `orchestrator/tests/test_stdout_router.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_stdout_router.py
import asyncio
import pytest
from unittest.mock import AsyncMock
from synapse_os.stdout_router import StdoutRouter

pytestmark = pytest.mark.asyncio


async def test_routes_stdout_lines():
    stdb = AsyncMock()
    router = StdoutRouter(stdb_client=stdb, project_id=1, stdb_agent_id=1)

    # Create a mock stream
    lines = [b"line 1\n", b"line 2\n", b""]
    read_calls = iter(lines)

    class FakeStream:
        async def readline(self):
            return next(read_calls)

    await router.route_stream(FakeStream(), agent_id="w-1")

    assert stdb.post_action_card.call_count == 2
    # Check the content of the first call
    call_args = stdb.post_action_card.call_args_list[0]
    assert call_args.kwargs["visual_type"] == "TerminalOutput"
    assert "line 1" in call_args.kwargs["content"]


async def test_batches_rapid_lines():
    stdb = AsyncMock()
    router = StdoutRouter(stdb_client=stdb, project_id=1, stdb_agent_id=1, batch_interval=0.1)

    lines = [b"line 1\n", b"line 2\n", b"line 3\n", b""]
    read_calls = iter(lines)

    class FakeStream:
        async def readline(self):
            return next(read_calls)

    await router.route_stream(FakeStream(), agent_id="w-1", batch=True)

    # With batching, all 3 lines should be in one card
    assert stdb.post_action_card.call_count == 1
    content = stdb.post_action_card.call_args.kwargs["content"]
    assert "line 1" in content
    assert "line 3" in content


async def test_empty_stream_no_cards():
    stdb = AsyncMock()
    router = StdoutRouter(stdb_client=stdb, project_id=1, stdb_agent_id=1)

    class FakeStream:
        async def readline(self):
            return b""

    await router.route_stream(FakeStream(), agent_id="w-1")
    stdb.post_action_card.assert_not_called()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_stdout_router.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write stdout router implementation**

```python
# synapse_os/stdout_router.py
from __future__ import annotations

from typing import Any, Protocol

from synapse_os.spacetimedb_client import SpacetimeDBClient


class ReadableStream(Protocol):
    async def readline(self) -> bytes: ...


class StdoutRouter:
    """Routes CLI subprocess stdout into SpacetimeDB ActionCards."""

    def __init__(
        self,
        stdb_client: SpacetimeDBClient,
        project_id: int,
        stdb_agent_id: int,
        batch_interval: float = 0.5,
    ) -> None:
        self._stdb = stdb_client
        self._project_id = project_id
        self._stdb_agent_id = stdb_agent_id
        self._batch_interval = batch_interval

    async def route_stream(
        self,
        stream: ReadableStream,
        agent_id: str,
        batch: bool = False,
    ) -> None:
        if batch:
            await self._route_batched(stream, agent_id)
        else:
            await self._route_line_by_line(stream, agent_id)

    async def _route_line_by_line(self, stream: ReadableStream, agent_id: str) -> None:
        while True:
            line = await stream.readline()
            if not line:
                break
            text = line.decode("utf-8", errors="replace").rstrip("\n")
            if text:
                await self._post_card(agent_id, text)

    async def _route_batched(self, stream: ReadableStream, agent_id: str) -> None:
        lines: list[str] = []
        while True:
            line = await stream.readline()
            if not line:
                break
            text = line.decode("utf-8", errors="replace").rstrip("\n")
            if text:
                lines.append(text)
        if lines:
            await self._post_card(agent_id, "\n".join(lines))

    async def _post_card(self, agent_id: str, content: str) -> None:
        await self._stdb.post_action_card(
            agent_id=self._stdb_agent_id,
            project_id=self._project_id,
            visual_type="TerminalOutput",
            content=content,
            task_summary=f"Output from {agent_id}",
            priority=0,
        )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_stdout_router.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/stdout_router.py orchestrator/tests/test_stdout_router.py
git commit -m "feat(phase1): stdout router pipes CLI output into SpacetimeDB ActionCards"
```

---

### Task 8: Health Checker

**Files:**
- Create: `orchestrator/synapse_os/health_checker.py`
- Create: `orchestrator/tests/test_health_checker.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_health_checker.py
import pytest
from unittest.mock import AsyncMock, MagicMock
from synapse_os.brain import Brain
from synapse_os.models import AgentRole, AgentState
from synapse_os.registry import AgentRegistry
from synapse_os.health_checker import HealthChecker

pytestmark = pytest.mark.asyncio


async def test_detects_stale_agents(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    pm = AsyncMock()

    checker = HealthChecker(registry=registry, process_manager=pm, brain=brain, heartbeat_timeout=60, blocked_threshold=1800)

    # Register agent with very old heartbeat
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 100)
    await redis.hset(brain.key_agent_state("w-1"), "last_heartbeat", "1000")

    stale = await checker.check_heartbeats()
    assert "w-1" in [s.agent_id for s in stale]


async def test_detects_crashed_processes(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)

    crashed_proc = MagicMock()
    crashed_proc.agent_id = "w-1"
    crashed_proc.command = ["claude", "--cli"]
    crashed_proc.cwd = "/tmp"

    pm = AsyncMock()
    pm.get_crashed.return_value = [crashed_proc]

    checker = HealthChecker(registry=registry, process_manager=pm, brain=brain, heartbeat_timeout=60, blocked_threshold=1800)

    crashed = checker.check_crashed()
    assert len(crashed) == 1
    assert crashed[0].agent_id == "w-1"


async def test_detects_blocked_agents(redis):
    brain = Brain(redis)
    registry = AgentRegistry(brain, redis)
    pm = AsyncMock()

    checker = HealthChecker(registry=registry, process_manager=pm, brain=brain, heartbeat_timeout=60, blocked_threshold=30)

    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 100)
    await registry.update_status("w-1", "blocked")
    # Set heartbeat to old time (simulating blocked for a long time)
    await redis.hset(brain.key_agent_state("w-1"), "last_heartbeat", "1000")

    blocked = await checker.check_blocked()
    assert "w-1" in [a.agent_id for a in blocked]
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_health_checker.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write health checker implementation**

```python
# synapse_os/health_checker.py
from __future__ import annotations

import time

from synapse_os.brain import Brain
from synapse_os.models import AgentState
from synapse_os.process_manager import ManagedProcess, ProcessManager
from synapse_os.registry import AgentRegistry


class HealthChecker:
    """Monitors agent health via heartbeats and process status."""

    def __init__(
        self,
        registry: AgentRegistry,
        process_manager: ProcessManager,
        brain: Brain,
        heartbeat_timeout: int,
        blocked_threshold: int,
    ) -> None:
        self._registry = registry
        self._pm = process_manager
        self._brain = brain
        self._heartbeat_timeout = heartbeat_timeout
        self._blocked_threshold = blocked_threshold

    async def check_heartbeats(self) -> list[AgentState]:
        """Return agents whose heartbeat is older than timeout."""
        return await self._registry.get_stale_agents(self._heartbeat_timeout)

    def check_crashed(self) -> list[ManagedProcess]:
        """Return processes that have exited unexpectedly."""
        return self._pm.get_crashed()

    async def check_blocked(self) -> list[AgentState]:
        """Return agents that have been in 'blocked' status too long."""
        agents = await self._registry.list_agents()
        now = int(time.time())
        blocked = []
        for agent in agents:
            if agent.status == "blocked":
                blocked_duration = now - agent.last_heartbeat
                if blocked_duration > self._blocked_threshold:
                    blocked.append(agent)
        return blocked
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_health_checker.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/health_checker.py orchestrator/tests/test_health_checker.py
git commit -m "feat(phase1): health checker for heartbeat, crash, and blocker detection"
```

---

### Task 9: Cron Engine

**Files:**
- Create: `orchestrator/synapse_os/cron_engine.py`
- Create: `orchestrator/tests/test_cron_engine.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_cron_engine.py
import asyncio
import pytest
from synapse_os.cron_engine import CronEngine

pytestmark = pytest.mark.asyncio


async def test_registers_and_fires_job():
    fired = []

    async def my_job():
        fired.append(True)

    engine = CronEngine()
    engine.register("test_job", interval_seconds=0.1, callback=my_job)
    task = asyncio.create_task(engine.run())

    await asyncio.sleep(0.35)
    engine.stop()
    await task

    # Should have fired at least 2 times in 0.35s with 0.1s interval
    assert len(fired) >= 2


async def test_multiple_jobs_different_intervals():
    fast_count = []
    slow_count = []

    async def fast_job():
        fast_count.append(1)

    async def slow_job():
        slow_count.append(1)

    engine = CronEngine()
    engine.register("fast", interval_seconds=0.05, callback=fast_job)
    engine.register("slow", interval_seconds=0.2, callback=slow_job)
    task = asyncio.create_task(engine.run())

    await asyncio.sleep(0.5)
    engine.stop()
    await task

    assert len(fast_count) > len(slow_count)


async def test_stop_is_clean():
    engine = CronEngine()
    async def noop():
        pass

    engine.register("noop", interval_seconds=0.1, callback=noop)
    task = asyncio.create_task(engine.run())
    engine.stop()
    await task  # Should not hang
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_cron_engine.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write cron engine implementation**

```python
# synapse_os/cron_engine.py
from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from typing import Awaitable, Callable

logger = logging.getLogger(__name__)


@dataclass
class CronJob:
    name: str
    interval_seconds: float
    callback: Callable[[], Awaitable[None]]


class CronEngine:
    """Simple async cron-like scheduler for recurring tasks."""

    def __init__(self) -> None:
        self._jobs: list[CronJob] = []
        self._running = False

    def register(self, name: str, interval_seconds: float, callback: Callable[[], Awaitable[None]]) -> None:
        self._jobs.append(CronJob(name=name, interval_seconds=interval_seconds, callback=callback))

    def stop(self) -> None:
        self._running = False

    async def run(self) -> None:
        self._running = True
        tasks = [asyncio.create_task(self._run_job(job)) for job in self._jobs]
        # Wait until stopped
        while self._running:
            await asyncio.sleep(0.05)
        # Cancel all job tasks
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)

    async def _run_job(self, job: CronJob) -> None:
        while self._running:
            try:
                await job.callback()
            except Exception:
                logger.exception("Cron job %s failed", job.name)
            await asyncio.sleep(job.interval_seconds)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_cron_engine.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/cron_engine.py orchestrator/tests/test_cron_engine.py
git commit -m "feat(phase1): async cron engine for standup triggers and health checks"
```

---

### Task 10: Feedback Watcher (SpacetimeDB → Redis)

**Files:**
- Create: `orchestrator/synapse_os/feedback_watcher.py`
- Create: `orchestrator/tests/test_feedback_watcher.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_feedback_watcher.py
import json
import pytest
from unittest.mock import AsyncMock
from synapse_os.brain import Brain
from synapse_os.feedback_watcher import FeedbackWatcher

pytestmark = pytest.mark.asyncio


async def test_forwards_escalation_response(redis):
    brain = Brain(redis)
    stdb = AsyncMock()

    # Pre-register escalation card → manager mapping
    await redis.set(brain.key_escalation_card("10"), "mgr-1")

    # Mock feedback query: card_id=10, action_type=approve
    stdb.query_feedback_since.return_value = [
        [1, 10, "approve", "", 2000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    await watcher.poll_once(since_micros=1000000)

    # Should have forwarded to manager inbox
    msg_raw = await brain.pop_manager_inbox("mgr-1", timeout=1)
    assert msg_raw is not None
    msg = json.loads(msg_raw)
    assert msg["msg_type"] == "escalation_response"
    assert msg["payload"]["approved"] is True
    assert msg["payload"]["card_id"] == 10


async def test_forwards_reject_with_comment(redis):
    brain = Brain(redis)
    stdb = AsyncMock()
    await redis.set(brain.key_escalation_card("11"), "mgr-2")

    stdb.query_feedback_since.return_value = [
        [2, 11, "reject", "needs more testing", 2000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    await watcher.poll_once(since_micros=1000000)

    msg_raw = await brain.pop_manager_inbox("mgr-2", timeout=1)
    msg = json.loads(msg_raw)
    assert msg["payload"]["approved"] is False
    assert msg["payload"]["comment"] == "needs more testing"


async def test_ignores_non_escalation_feedback(redis):
    brain = Brain(redis)
    stdb = AsyncMock()

    # No escalation_card mapping exists for card_id 99
    stdb.query_feedback_since.return_value = [
        [3, 99, "comment", "looks good", 2000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    await watcher.poll_once(since_micros=1000000)

    # No manager inbox should have messages
    msg = await redis.blpop("manager_inbox:mgr-1", timeout=0.1)
    assert msg is None


async def test_tracks_high_water_mark(redis):
    brain = Brain(redis)
    stdb = AsyncMock()
    await redis.set(brain.key_escalation_card("10"), "mgr-1")

    stdb.query_feedback_since.return_value = [
        [1, 10, "approve", "", 5000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    new_hwm = await watcher.poll_once(since_micros=1000000)
    assert new_hwm == 5000000
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_feedback_watcher.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write feedback watcher implementation**

```python
# synapse_os/feedback_watcher.py
from __future__ import annotations

import json
import logging

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.models import InboxMessage
from synapse_os.spacetimedb_client import SpacetimeDBClient

logger = logging.getLogger(__name__)


class FeedbackWatcher:
    """Polls SpacetimeDB feedback table and forwards escalation responses to manager inboxes."""

    def __init__(self, brain: Brain, redis: Redis, stdb_client: SpacetimeDBClient) -> None:
        self._brain = brain
        self._r = redis
        self._stdb = stdb_client

    async def poll_once(self, since_micros: int) -> int:
        """Poll for new feedback since the given timestamp. Returns the new high-water mark."""
        rows = await self._stdb.query_feedback_since(since_micros)
        high_water = since_micros

        for row in rows:
            feedback_id, card_id, action_type, payload, created_at = row

            # Track high-water mark
            if created_at > high_water:
                high_water = created_at

            # Look up if this card is an escalation
            manager_id = await self._r.get(self._brain.key_escalation_card(str(card_id)))
            if not manager_id:
                continue  # Not an escalation card — skip

            # Forward to manager inbox
            approved = action_type == "approve"
            msg = InboxMessage(
                msg_type="escalation_response",
                payload={
                    "approved": approved,
                    "comment": payload or "",
                    "card_id": card_id,
                },
                sender="human",
            )
            await self._brain.push_manager_inbox(manager_id, msg.to_json())
            logger.info("Forwarded escalation response for card %s to manager %s", card_id, manager_id)

        return high_water
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_feedback_watcher.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add orchestrator/synapse_os/feedback_watcher.py orchestrator/tests/test_feedback_watcher.py
git commit -m "feat(phase1): feedback watcher forwards SpacetimeDB escalations to Redis"
```

---

### Task 11: Main Daemon (Wire Everything Together)

**Files:**
- Create: `orchestrator/synapse_os/daemon.py`
- Create: `orchestrator/tests/test_daemon.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_daemon.py
import asyncio
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.config import Config
from synapse_os.daemon import Daemon

pytestmark = pytest.mark.asyncio


async def test_daemon_starts_and_stops(redis):
    cfg = Config()
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    # Run for a short time then stop
    task = asyncio.create_task(daemon.run())
    await asyncio.sleep(0.3)
    daemon.shutdown()
    await asyncio.wait_for(task, timeout=5.0)


async def test_daemon_registers_cron_jobs(redis):
    cfg = Config()
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    # Verify cron jobs are registered
    job_names = [j.name for j in daemon._cron._jobs]
    assert "health_check" in job_names
    assert "feedback_poll" in job_names
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd orchestrator && pytest tests/test_daemon.py -v`
Expected: FAIL — `ModuleNotFoundError`

- [ ] **Step 3: Write daemon implementation**

```python
# synapse_os/daemon.py
from __future__ import annotations

import asyncio
import logging
import signal
import sys

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.cron_engine import CronEngine
from synapse_os.feedback_watcher import FeedbackWatcher
from synapse_os.health_checker import HealthChecker
from synapse_os.process_manager import ProcessManager
from synapse_os.registry import AgentRegistry
from synapse_os.spacetimedb_client import SpacetimeDBClient

logger = logging.getLogger(__name__)


class Daemon:
    """Main orchestrator daemon. Wires all components and runs the event loop."""

    def __init__(
        self,
        config: Config,
        redis: Redis,
        stdb_client: SpacetimeDBClient,
    ) -> None:
        self._config = config
        self._redis = redis
        self._stdb = stdb_client

        # Components
        self._brain = Brain(redis)
        self._registry = AgentRegistry(self._brain, redis)
        self._pm = ProcessManager()
        self._health = HealthChecker(
            registry=self._registry,
            process_manager=self._pm,
            brain=self._brain,
            heartbeat_timeout=config.heartbeat_timeout_seconds,
            blocked_threshold=config.blocked_threshold_seconds,
        )
        self._feedback = FeedbackWatcher(
            brain=self._brain,
            redis=redis,
            stdb_client=stdb_client,
        )
        self._cron = CronEngine()
        self._feedback_hwm: int = 0
        self._shutting_down = False

        # Register cron jobs
        self._cron.register("health_check", config.heartbeat_timeout_seconds, self._health_check_tick)
        self._cron.register("feedback_poll", config.feedback_poll_interval_seconds, self._feedback_poll_tick)

    async def run(self) -> None:
        logger.info("Synapse OS daemon starting")
        try:
            await self._cron.run()
        finally:
            await self._cleanup()

    def shutdown(self) -> None:
        logger.info("Shutdown requested")
        self._shutting_down = True
        self._cron.stop()

    async def _cleanup(self) -> None:
        logger.info("Cleaning up: killing all agent processes")
        await self._pm.kill_all()
        await self._stdb.close()
        logger.info("Daemon stopped")

    async def _health_check_tick(self) -> None:
        # Check for crashed processes
        crashed = self._health.check_crashed()
        for proc in crashed:
            logger.warning("Agent %s crashed (pid %d), removing from registry", proc.agent_id, proc.pid)
            await self._registry.deregister(proc.agent_id)
            self._pm.processes.pop(proc.agent_id, None)

        # Check for stale heartbeats
        stale = await self._health.check_heartbeats()
        for agent in stale:
            logger.warning("Agent %s heartbeat stale", agent.agent_id)

        # Check for blocked agents — notify their managers
        blocked = await self._health.check_blocked()
        for agent in blocked:
            if agent.manager_id:
                from synapse_os.models import InboxMessage
                msg = InboxMessage(
                    msg_type="agent_blocked",
                    payload={"agent_id": agent.agent_id, "status": agent.status},
                    sender="orchestrator",
                )
                await self._brain.push_manager_inbox(agent.manager_id, msg.to_json())

    async def _feedback_poll_tick(self) -> None:
        self._feedback_hwm = await self._feedback.poll_once(self._feedback_hwm)


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(name)s] %(levelname)s: %(message)s")

    config = Config.from_env()

    async def _run() -> None:
        redis = Redis.from_url(config.redis_url, decode_responses=True)
        stdb = SpacetimeDBClient(config)
        daemon = Daemon(config=config, redis=redis, stdb_client=stdb)

        loop = asyncio.get_event_loop()
        loop.add_signal_handler(signal.SIGINT, daemon.shutdown)
        loop.add_signal_handler(signal.SIGTERM, daemon.shutdown)

        await daemon.run()
        await redis.aclose()

    asyncio.run(_run())


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd orchestrator && pytest tests/test_daemon.py -v`
Expected: 2 passed

- [ ] **Step 5: Run all tests to verify nothing is broken**

Run: `cd orchestrator && pytest tests/ -v`
Expected: All tests pass (31 total)

- [ ] **Step 6: Commit**

```bash
git add orchestrator/synapse_os/daemon.py orchestrator/tests/test_daemon.py
git commit -m "feat(phase1): main daemon wires orchestrator components with cron + graceful shutdown"
```

---

### Task 12: Integration Smoke Test

**Files:**
- Create: `orchestrator/tests/test_integration.py`

- [ ] **Step 1: Write integration test**

```python
# tests/test_integration.py
"""Integration test: exercises the full flow with fakeredis and mocked SpacetimeDB."""
import asyncio
import json
import pytest
from unittest.mock import AsyncMock

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.daemon import Daemon
from synapse_os.models import AgentRole, InboxMessage

pytestmark = pytest.mark.asyncio


async def test_full_lifecycle(redis):
    """Register agent → enqueue ticket → claim → complete → feedback forward."""
    cfg = Config()
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)
    brain = daemon._brain
    registry = daemon._registry

    # 1. Register a manager and a worker
    await registry.register("m-1", AgentRole.MANAGER, "frontend", "", 100)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 200)

    # 2. Create a ticket and enqueue it
    from synapse_os.models import Ticket, TicketStatus
    ticket = Ticket(
        ticket_id="t-1",
        title="Build login page",
        description="Create login form with email/password",
        domain="frontend",
        priority=1,
        status=TicketStatus.PENDING,
    )
    await redis.hset(brain.key_ticket("t-1"), mapping=ticket.to_dict())
    await brain.enqueue_ticket("t-1")

    # 3. Worker claims the ticket
    claimed = await brain.claim_ticket(timeout=1)
    assert claimed == "t-1"
    result = await brain.register_claim("t-1", "w-1")
    assert result is True

    # Verify state
    assert await redis.hget("in_progress", "t-1") == "w-1"
    assert await redis.hget(brain.key_ticket("t-1"), "status") == "in_progress"

    # 4. Worker completes and notifies manager
    completion = InboxMessage(
        msg_type="completion",
        payload={"ticket_id": "t-1", "pr_url": "https://github.com/org/repo/pull/1"},
        sender="w-1",
    )
    await brain.push_manager_inbox("m-1", completion.to_json())

    # 5. Manager reads completion
    msg_raw = await brain.pop_manager_inbox("m-1", timeout=1)
    msg = InboxMessage.from_json(msg_raw)
    assert msg.msg_type == "completion"
    assert msg.payload["pr_url"] == "https://github.com/org/repo/pull/1"

    # 6. Simulate escalation feedback forwarding
    await redis.set(brain.key_escalation_card("42"), "m-1")
    stdb.query_feedback_since.return_value = [
        [1, 42, "approve", "", 9999999],
    ]
    new_hwm = await daemon._feedback.poll_once(0)
    assert new_hwm == 9999999

    esc_msg_raw = await brain.pop_manager_inbox("m-1", timeout=1)
    esc_msg = InboxMessage.from_json(esc_msg_raw)
    assert esc_msg.msg_type == "escalation_response"
    assert esc_msg.payload["approved"] is True
```

- [ ] **Step 2: Run integration test**

Run: `cd orchestrator && pytest tests/test_integration.py -v`
Expected: PASS

- [ ] **Step 3: Run full test suite**

Run: `cd orchestrator && pytest tests/ -v --tb=short`
Expected: All tests pass (32 total)

- [ ] **Step 4: Commit**

```bash
git add orchestrator/tests/test_integration.py
git commit -m "test(phase1): integration smoke test for full orchestrator lifecycle"
```

---

### Task 13: Update start.sh and Documentation

**Files:**
- Modify: `start.sh`
- Modify: `README.md` (add orchestrator section)

- [ ] **Step 1: Update start.sh to include Redis check**

Add a Redis availability check near the top of `start.sh`, after the SpacetimeDB start:

```bash
# Check Redis is running
if ! redis-cli ping > /dev/null 2>&1; then
    echo "⚠ Redis not running. Starting Redis..."
    redis-server --daemonize yes
fi
echo "✓ Redis connected"
```

- [ ] **Step 2: Add orchestrator start command to start.sh**

After the worker start section, add:

```bash
# Start Synapse OS orchestrator
echo "Starting Synapse OS orchestrator..."
cd "$PROJECT_ROOT/orchestrator"
pip3 install -e . --quiet --break-system-packages 2>/dev/null
synapse-os > /tmp/synapse-os.log 2>&1 &
ORCHESTRATOR_PID=$!
echo "✓ Orchestrator running (PID: $ORCHESTRATOR_PID)"
```

- [ ] **Step 3: Run start.sh to verify it doesn't error on the new sections**

Run: `./start.sh` (manual verification — check logs at `/tmp/synapse-os.log`)

- [ ] **Step 4: Commit**

```bash
git add start.sh
git commit -m "feat(phase1): integrate orchestrator daemon into start.sh"
```
