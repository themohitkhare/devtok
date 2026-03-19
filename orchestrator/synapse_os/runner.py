"""Run flow: start manager + workers, monitor execution."""
from __future__ import annotations

import asyncio
import json
import logging
from typing import Optional

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.models import AgentRole, InboxMessage
from synapse_os.prompts.manager import build_manager_prompt
from synapse_os.prompts.worker import build_worker_prompt
from synapse_os.registry import AgentRegistry
from synapse_os.spawner import AgentSpawner

logger = logging.getLogger(__name__)


class Runner:
    """Manages a manager agent + N worker agents for a domain.

    The manager runs in a loop: each cycle spawns a Claude Code instance that
    checks the inbox, assigns tickets, and reviews work. Workers are spawned
    per-ticket when the manager assigns them.
    """

    def __init__(self, config: Config, project_dir: str, redis: Redis, brain: Brain,
                 domain: str = "general", num_workers: int = 2,
                 manager_cycle_seconds: int = 30) -> None:
        self._config = config
        self._project_dir = project_dir
        self._redis = redis
        self._brain = brain
        self._domain = domain
        self._num_workers = num_workers
        self._manager_cycle_seconds = manager_cycle_seconds
        self._spawner = AgentSpawner(config=config, project_dir=project_dir)
        self._registry = AgentRegistry(brain, redis)
        self._manager_id = f"mgr-{domain}"
        self._worker_ids: list[str] = []
        self._running = False

    async def start(self) -> None:
        """Register manager and workers. Call run_loop() separately to start the manager cycle."""
        logger.info("Starting runner for domain=%s with %d workers", self._domain, self._num_workers)
        self._running = True

        # Register manager in registry
        await self._registry.register(self._manager_id, AgentRole.MANAGER, self._domain, "", 0)

        # Register workers
        for i in range(self._num_workers):
            worker_id = f"w-{self._domain}-{i}"
            self._worker_ids.append(worker_id)
            await self._registry.register(worker_id, AgentRole.WORKER, self._domain, self._manager_id, 0)
            logger.info("Worker %s registered", worker_id)

    async def run_loop(self) -> None:
        """Run the manager loop. Blocks until stop() is called."""
        await self._manager_loop()

    async def _manager_loop(self) -> None:
        """Run the manager in cycles. Each cycle spawns Claude Code, which
        checks the work queue, reads inbox, assigns tickets, and exits."""
        cycle = 0
        while self._running:
            cycle += 1
            logger.info("Manager cycle %d starting", cycle)

            # Build context about current state for the manager
            pending = await self.get_pending_tickets()
            worker_states = []
            for wid in self._worker_ids:
                state = await self._registry.get_state(wid)
                if state:
                    worker_states.append(f"  {wid}: status={state.status}, ticket={state.current_ticket or 'none'}")

            # Check manager inbox for messages
            inbox_messages = []
            while True:
                msg_raw = await self._brain.pop_manager_inbox(self._manager_id, timeout=1)
                if not msg_raw:
                    break
                inbox_messages.append(msg_raw)

            state_summary = (
                f"## Current State (Cycle {cycle})\n\n"
                f"**Pending tickets in queue:** {len(pending)}\n"
            )
            for t in pending:
                state_summary += f"  - {t.get('ticket_id', '?')}: {t.get('title', '?')} (priority={t.get('priority', '?')})\n"

            state_summary += f"\n**Workers ({len(self._worker_ids)}):**\n"
            state_summary += "\n".join(worker_states) if worker_states else "  (no state available)\n"

            if inbox_messages:
                state_summary += f"\n**Inbox messages ({len(inbox_messages)}):**\n"
                for msg_raw in inbox_messages:
                    try:
                        msg = InboxMessage.from_json(msg_raw)
                        state_summary += f"  - [{msg.msg_type}] from {msg.sender}: {json.dumps(msg.payload)[:100]}\n"
                    except Exception:
                        state_summary += f"  - (unparseable): {msg_raw[:100]}\n"
            else:
                state_summary += "\n**Inbox:** empty\n"

            # Only run manager if there's work to do
            if not pending and not inbox_messages:
                logger.info("Manager cycle %d: nothing to do, sleeping %ds", cycle, self._manager_cycle_seconds)
                await asyncio.sleep(self._manager_cycle_seconds)
                continue

            manager_prompt = build_manager_prompt(domain=self._domain)
            task_prompt = (
                f"You are the {self._domain} manager. Here is the current project state:\n\n"
                f"{state_summary}\n\n"
                f"Your available workers: {', '.join(self._worker_ids)}\n\n"
                "Based on this state:\n"
                "1. If there are pending tickets, assign them to idle workers using assign_ticket\n"
                "2. If there are completion messages, mark those tickets as completed using update_ticket\n"
                "3. If workers are blocked, try to help them\n"
                "4. Post a brief status_card summarizing what you did\n"
            )

            try:
                proc = await self._spawner.spawn_agent(
                    agent_id=f"{self._manager_id}-cycle-{cycle}",
                    role="manager",
                    prompt=task_prompt,
                    system_prompt=manager_prompt,
                )
                logger.info("Manager cycle %d spawned (pid=%d)", cycle, proc.pid)

                # Wait for this cycle to complete (with timeout)
                try:
                    await asyncio.wait_for(proc.process.wait(), timeout=120)
                except asyncio.TimeoutError:
                    logger.warning("Manager cycle %d timed out, killing", cycle)
                    await self._spawner.kill_agent(f"{self._manager_id}-cycle-{cycle}")

            except Exception as e:
                logger.error("Manager cycle %d failed: %s", cycle, e)

            # Now check if any tickets were assigned and spawn workers for them
            await self._check_and_spawn_workers()

            logger.info("Manager cycle %d complete, sleeping %ds", cycle, self._manager_cycle_seconds)
            await asyncio.sleep(self._manager_cycle_seconds)

    async def _check_and_spawn_workers(self) -> None:
        """Check worker inboxes for ticket assignments and spawn Claude Code for each."""
        for worker_id in self._worker_ids:
            state = await self._registry.get_state(worker_id)
            if state and state.status != "idle":
                continue  # Already working

            # Check if there's a ticket assignment in the worker's inbox
            msg_raw = await self._brain.pop_inbox(worker_id, timeout=1)
            if not msg_raw:
                continue

            try:
                msg = InboxMessage.from_json(msg_raw)
                if msg.msg_type != "ticket_assignment":
                    continue

                ticket_id = msg.payload.get("ticket_id", "")
                title = msg.payload.get("title", "")
                description = msg.payload.get("description", "")

                logger.info("Spawning worker %s for ticket %s: %s", worker_id, ticket_id, title)

                # Update worker state
                await self._registry.update_status(worker_id, "working", current_ticket=ticket_id)

                # Build worker-specific prompt
                worker_system_prompt = build_worker_prompt(
                    ticket_id=ticket_id, title=title, description=description, domain=self._domain,
                )
                worker_task_prompt = (
                    f"You have been assigned ticket {ticket_id}: {title}\n\n"
                    f"Description: {description}\n\n"
                    "Execute this ticket now. Follow the process in your system prompt."
                )

                proc = await self._spawner.spawn_agent(
                    agent_id=f"{worker_id}-{ticket_id}",
                    role="worker",
                    prompt=worker_task_prompt,
                    system_prompt=worker_system_prompt,
                    manager_id=self._manager_id,
                )
                logger.info("Worker %s spawned for ticket %s (pid=%d)", worker_id, ticket_id, proc.pid)

                # Wait for worker to finish (with timeout)
                try:
                    await asyncio.wait_for(proc.process.wait(), timeout=300)
                    logger.info("Worker %s completed ticket %s", worker_id, ticket_id)
                except asyncio.TimeoutError:
                    logger.warning("Worker %s timed out on ticket %s", worker_id, ticket_id)
                    await self._spawner.kill_agent(f"{worker_id}-{ticket_id}")

                # Reset worker state
                await self._registry.update_status(worker_id, "idle", current_ticket="")

            except Exception as e:
                logger.error("Failed to spawn worker %s: %s", worker_id, e)

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
        self._running = False
        await self._spawner.kill_all()
        for wid in self._worker_ids:
            await self._registry.deregister(wid)
        await self._registry.deregister(self._manager_id)
        logger.info("Runner stopped for domain=%s", self._domain)
