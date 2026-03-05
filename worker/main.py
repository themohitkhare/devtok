import os
import sys
import asyncio
import random
import time
import signal
import json

import httpx


# Ensure we can import from the local src/ directory when running `python main.py`.
CURRENT_DIR = os.path.dirname(os.path.abspath(__file__))
SRC_DIR = os.path.join(CURRENT_DIR, "src")
if SRC_DIR not in sys.path:
    sys.path.insert(0, SRC_DIR)

from generate_mock_content import (  # type: ignore  # noqa: E402
    get_random_diff,
    get_random_terminal,
    get_random_summary,
)


STDB_BASE = "http://localhost:3000"
MODULE = "synapse"


async def call_reducer(client: httpx.AsyncClient, reducer: str, args: list) -> dict:
    """
    Call a SpacetimeDB reducer via the HTTP API.
    """
    url = f"{STDB_BASE}/v1/database/{MODULE}/call/{reducer}"
    headers = {
        "Authorization": "Bearer",
        "Content-Type": "application/json",
    }

    try:
        resp = await client.post(url, headers=headers, content=json.dumps(args))
    except httpx.HTTPError as exc:
        print(f"[call_reducer] HTTP error calling {reducer}: {exc}")
        return {}

    if resp.status_code != 200:
        print(
            f"[call_reducer] Non-200 status from {reducer}: "
            f"{resp.status_code} {resp.text}"
        )
        # Try to return JSON if possible so callers can still inspect it
        try:
            return resp.json()
        except json.JSONDecodeError:
            return {}

    try:
        return resp.json()
    except json.JSONDecodeError as exc:
        print(f"[call_reducer] Failed to decode JSON from {reducer}: {exc}")
        return {}


async def seed_agents(client: httpx.AsyncClient) -> list[int]:
    """
    Create a small set of agents in the Agent table and return their IDs.
    """

    agents = [
        ("Frontend UI Agent", "React/TypeScript/CSS", "frontend-001"),
        ("DB Migration Agent", "PostgreSQL/Schema", "db-002"),
        ("Security Scanner", "Dependency Audit/CVE", "sec-003"),
    ]

    agent_ids: list[int] = []

    for name, specialty, avatar_seed in agents:
        payload = [name, specialty, avatar_seed]
        result = await call_reducer(client, "create_agent", payload)

        agent_id = None
        if isinstance(result, dict):
            # Try a couple of common shapes
            if "id" in result and isinstance(result["id"], int):
                agent_id = result["id"]
            elif "agent_id" in result and isinstance(result["agent_id"], int):
                agent_id = result["agent_id"]

        if agent_id is None:
            # Fall back to a synthetic ID so the worker can still run locally,
            # even if the reducer response shape is different.
            agent_id = len(agent_ids) + 1
            print(
                f"[seed_agents] Could not infer agent id from reducer response, "
                f"falling back to synthetic id {agent_id}. Raw result: {result}"
            )

        agent_ids.append(agent_id)
        print(f"Seeded agent '{name}' with id={agent_id}")

    return agent_ids


def print_banner() -> None:
    banner = r"""
  _____ __   __ _   _  _____  _____  _____
 /  ___|\ \ / /| \ | |/  ___||  _  ||  ___|
 \ `--.  \ V / |  \| |\ `--. | | | || |__
  `--. \ /   \ | . ` | `--. \| | | ||  __|
 /\__/ /| |\  \| |\  |/\__/ /\ \_/ /| |___
 \____/ \_| \_/\_| \_/\____/  \___/ \____/

   Synapse Mock Agent Worker
"""
    print(banner)


async def main() -> None:
    running = True

    loop = asyncio.get_running_loop()

    def _handle_signal(sig: signal.Signals) -> None:
        nonlocal running
        print(f"\nReceived signal {sig.name}, shutting down gracefully...")
        running = False

    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(sig, _handle_signal, sig)
        except NotImplementedError:
            # add_signal_handler is not available on some platforms (e.g. Windows)
            pass

    print_banner()

    async with httpx.AsyncClient(timeout=10.0) as client:
        agent_ids = await seed_agents(client)

        if not agent_ids:
            print("No agents available, exiting.")
            return

        running_tasks: list[int] = []

        print("Starting mock event loop. Press Ctrl+C to stop.")

        while running:
            wait = random.uniform(8, 15)
            await asyncio.sleep(wait)

            agent_id = random.choice(agent_ids)
            visual_type = "CodeDiff" if random.random() < 0.6 else "TerminalOutput"

            if visual_type == "CodeDiff":
                content = get_random_diff()
            else:
                content = get_random_terminal()

            summary = get_random_summary(visual_type)
            priority = random.randint(1, 10)

            await call_reducer(
                client,
                "insert_action_card",
                [agent_id, 1, visual_type, content, summary, priority],
            )
            print(
                f'[{time.strftime("%H:%M:%S")}] Inserted {visual_type} card for '
                f"agent {agent_id}: {summary[:60]}..."
            )

            if random.random() < 0.3:
                task_types = [
                    "code",
                    "test",
                    "deploy",
                    "review",
                    "scan",
                    "migrate",
                    "refactor",
                ]
                task_type = random.choice(task_types)
                result = await call_reducer(
                    client,
                    "insert_concurrent_task",
                    [agent_id, task_type],
                )

                task_id = None
                if isinstance(result, dict):
                    if "id" in result and isinstance(result["id"], int):
                        task_id = result["id"]
                    elif "task_id" in result and isinstance(result["task_id"], int):
                        task_id = result["task_id"]

                if task_id is not None:
                    running_tasks.append(task_id)

                print(
                    f'[{time.strftime("%H:%M:%S")}] Started concurrent {task_type} '
                    f"task for agent {agent_id}"
                )


if __name__ == "__main__":
    asyncio.run(main())

#!/usr/bin/env python3
"""
Synapse mock agent worker — simulates multiple AI CLI workers posting ActionCards
and ConcurrentTasks to SpacetimeDB (module 'synapse').
"""

import random
import signal
import sys
import time
from typing import Any

import httpx

from generate_mock_content import (
    MOCK_CODE_DIFFS,
    MOCK_TERMINAL_OUTPUTS,
    MOCK_TASK_SUMMARIES,
)

# --- Config ---
SPACETIMEDB_BASE = "http://localhost:3000"
MODULE_NAME = "synapse"
INITIAL_AGENTS = [
    {"name": "Frontend UI Agent", "specialty": "React/TypeScript/CSS", "avatar_seed": "frontend-001"},
    {"name": "DB Migration Agent", "specialty": "PostgreSQL/Schema", "avatar_seed": "db-002"},
    {"name": "Security Scanner", "specialty": "Dependency Audit/CVE", "avatar_seed": "sec-003"},
]
ACTION_INTERVAL_MIN = 8
ACTION_INTERVAL_MAX = 15
CONCURRENT_TASK_TYPES = ["code", "test", "deploy", "review", "scan", "migrate", "refactor"]
CONCURRENT_TASK_COLORS = ["#0ea5e9", "#6366f1", "#10b981", "#f59e0b", "#ef4444"]


class SpacetimeDBError(Exception):
    """Raised when a SpacetimeDB reducer call fails."""
    pass


class SpacetimeDBClient:
    """Client for SpacetimeDB REST API (no Python SDK)."""

    def __init__(self, base_url: str = SPACETIMEDB_BASE, module_name: str = MODULE_NAME):
        self.base_url = base_url.rstrip("/")
        self.module_name = module_name
        self._client: httpx.Client | None = None

    def _get_client(self) -> httpx.Client:
        if self._client is None or self._client.is_closed:
            self._client = httpx.Client(
                base_url=self.base_url,
                timeout=30.0,
                headers={
                    "Authorization": "Bearer ",
                    "Content-Type": "application/json",
                },
            )
        return self._client

    def call_reducer(self, reducer_name: str, *args: Any) -> dict[str, Any] | list[Any] | None:
        """Call a reducer via POST /v1/database/{module}/call/{reducer}. Body is JSON array of args."""
        url = f"/v1/database/{self.module_name}/call/{reducer_name}"
        body = list(args) if args else []
        client = self._get_client()
        try:
            resp = client.post(url, json=body)
            resp.raise_for_status()
            if resp.content:
                return resp.json()
            return None
        except httpx.HTTPStatusError as e:
            raise SpacetimeDBError(f"SpacetimeDB {reducer_name}: HTTP {e.response.status_code}") from e
        except httpx.RequestError as e:
            raise SpacetimeDBError(f"SpacetimeDB {reducer_name}: {e}") from e

    def close(self) -> None:
        if self._client and not self._client.is_closed:
            self._client.close()
            self._client = None


def create_agents(client: SpacetimeDBClient) -> list[int]:
    """Create initial agents and return their IDs (or [1,2,3] if IDs not returned)."""
    ids: list[int] = []
    for a in INITIAL_AGENTS:
        try:
            out = client.call_reducer("create_agent", a["name"], a["specialty"], a["avatar_seed"])
            if isinstance(out, dict) and "id" in out:
                ids.append(int(out["id"]))
            elif isinstance(out, (list, tuple)) and len(out) > 0:
                ids.append(int(out[0]))
            else:
                ids.append(len(ids) + 1)
            print(f"[create_agent] {a['name']} -> id={ids[-1]}")
        except SpacetimeDBError as e:
            print(f"[create_agent] ERROR: {e}", file=sys.stderr)
            ids.append(len(ids) + 1)
    return ids if ids else [1, 2, 3]


def insert_action_card(
    client: SpacetimeDBClient,
    agent_id: int,
    visual_type: str,
    content: str,
    task_summary: str,
    status: str = "running",
    project_id: int = 1,
    priority: int = 0,
) -> None:
    """Call insert_action_card reducer. Timestamps can be set by the module."""
    now_ms = int(time.time() * 1000)
    client.call_reducer(
        "insert_action_card",
        agent_id,
        status,
        visual_type,
        content,
        task_summary,
        project_id,
        priority,
        now_ms,
        now_ms,
    )


def insert_concurrent_task(
    client: SpacetimeDBClient,
    agent_id: int,
    task_type: str,
    status: str = "running",
    color: str = "#0ea5e9",
) -> int | None:
    """Call insert_concurrent_task; return new task id if present in response."""
    now_ms = int(time.time() * 1000)
    out = client.call_reducer(
        "insert_concurrent_task",
        agent_id,
        task_type,
        status,
        color,
        now_ms,
    )
    if isinstance(out, dict) and "id" in out:
        return int(out["id"])
    if isinstance(out, (list, tuple)) and len(out) > 0:
        return int(out[0])
    return None


def complete_task(client: SpacetimeDBClient, task_id: int) -> None:
    """Call complete_task reducer."""
    client.call_reducer("complete_task", task_id)


def run_loop(client: SpacetimeDBClient, agent_ids: list[int], running_task_ids: list[int]) -> None:
    """One iteration: maybe insert action card, maybe insert/complete concurrent task."""
    # 1. Insert a new ActionCard
    agent_id = random.choice(agent_ids)
    if random.random() < 0.6:
        visual_type = "CodeDiff"
        content = random.choice(MOCK_CODE_DIFFS)
    else:
        visual_type = "TerminalOutput"
        content = random.choice(MOCK_TERMINAL_OUTPUTS)
    summaries = MOCK_TASK_SUMMARIES.get(visual_type, MOCK_TASK_SUMMARIES["CodeDiff"])
    task_summary = random.choice(summaries)
    status = random.choice(["running", "thinking", "success"])
    try:
        insert_action_card(
            client, agent_id, visual_type, content, task_summary,
            status=status, project_id=1, priority=random.randint(0, 2),
        )
        print(f"[insert_action_card] agent_id={agent_id} visual_type={visual_type} status={status} summary={task_summary!r}")
    except SpacetimeDBError as e:
        print(f"[insert_action_card] ERROR: {e}", file=sys.stderr)

    # 2. 20% chance: insert concurrent task
    if random.random() < 0.20:
        agent_id = random.choice(agent_ids)
        task_type = random.choice(CONCURRENT_TASK_TYPES)
        color = random.choice(CONCURRENT_TASK_COLORS)
        try:
            tid = insert_concurrent_task(client, agent_id, task_type, "running", color)
            if tid is not None:
                running_task_ids.append(tid)
            print(f"[insert_concurrent_task] agent_id={agent_id} task_type={task_type} -> id={tid}")
        except SpacetimeDBError as e:
            print(f"[insert_concurrent_task] ERROR: {e}", file=sys.stderr)

    # 3. 10% chance: complete a random running task
    if random.random() < 0.10 and running_task_ids:
        task_id = random.choice(running_task_ids)
        running_task_ids.remove(task_id)
        try:
            complete_task(client, task_id)
            print(f"[complete_task] task_id={task_id}")
        except SpacetimeDBError as e:
            print(f"[complete_task] ERROR: {e}", file=sys.stderr)
            running_task_ids.append(task_id)


def main() -> None:
    shutdown = False

    def on_signal(_sig: int, _frame: object) -> None:
        nonlocal shutdown
        shutdown = True
        print("\n[worker] Shutting down...")

    signal.signal(signal.SIGINT, on_signal)
    signal.signal(signal.SIGTERM, on_signal)

    client = SpacetimeDBClient()
    try:
        print("Connecting to SpacetimeDB at", SPACETIMEDB_BASE, "module", MODULE_NAME)
        agent_ids = create_agents(client)
        running_task_ids: list[int] = []
        print("Starting main loop (ActionCard every 8–15s, random concurrent tasks). Ctrl+C to stop.")
        while not shutdown:
            run_loop(client, agent_ids, running_task_ids)
            delay = random.randint(ACTION_INTERVAL_MIN, ACTION_INTERVAL_MAX)
            for _ in range(delay):
                if shutdown:
                    break
                time.sleep(1)
    finally:
        client.close()
    print("Worker stopped.")


if __name__ == "__main__":
    main()
