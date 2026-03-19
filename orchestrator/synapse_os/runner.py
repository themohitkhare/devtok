"""Run flow: start manager + workers, monitor execution."""
from __future__ import annotations

import asyncio
import logging
from typing import Optional

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
    def __init__(self, config: Config, project_dir: str, redis: Redis, brain: Brain,
                 domain: str = "general", num_workers: int = 2) -> None:
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
        logger.info("Starting runner for domain=%s with %d workers", self._domain, self._num_workers)
        manager_prompt = build_manager_prompt(domain=self._domain)
        await self._spawner.spawn_agent(
            agent_id=self._manager_id, role="manager",
            prompt=f"You are the {self._domain} domain manager. Check your inbox for messages, review the work queue, and assign tickets to your workers.",
            system_prompt=manager_prompt,
        )
        await self._registry.register(self._manager_id, AgentRole.MANAGER, self._domain, "", 0)
        logger.info("Manager %s started", self._manager_id)

        for i in range(self._num_workers):
            worker_id = f"w-{self._domain}-{i}"
            self._worker_ids.append(worker_id)
            worker_prompt = build_worker_prompt(
                ticket_id="(will be assigned)", title="(awaiting assignment)",
                description="Wait for a ticket assignment from your manager.", domain=self._domain,
            )
            await self._spawner.spawn_agent(
                agent_id=worker_id, role="worker",
                prompt="Wait for ticket assignment. Check your inbox for a ticket_assignment message, then execute the described work.",
                system_prompt=worker_prompt, manager_id=self._manager_id,
            )
            await self._registry.register(worker_id, AgentRole.WORKER, self._domain, self._manager_id, 0)
            logger.info("Worker %s started", worker_id)

    async def get_pending_tickets(self) -> list[dict]:
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
        await self._spawner.kill_all()
        for wid in self._worker_ids:
            await self._registry.deregister(wid)
        await self._registry.deregister(self._manager_id)
        logger.info("Runner stopped for domain=%s", self._domain)
