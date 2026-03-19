from __future__ import annotations

import time

from synapse_os.brain import Brain
from synapse_os.models import AgentState
from synapse_os.process_manager import ManagedProcess, ProcessManager
from synapse_os.registry import AgentRegistry


class HealthChecker:
    def __init__(self, registry: AgentRegistry, process_manager: ProcessManager, brain: Brain,
                 heartbeat_timeout: int, blocked_threshold: int) -> None:
        self._registry = registry
        self._pm = process_manager
        self._brain = brain
        self._heartbeat_timeout = heartbeat_timeout
        self._blocked_threshold = blocked_threshold

    async def check_heartbeats(self) -> list[AgentState]:
        return await self._registry.get_stale_agents(self._heartbeat_timeout)

    def check_crashed(self) -> list[ManagedProcess]:
        return self._pm.get_crashed()

    async def check_blocked(self) -> list[AgentState]:
        agents = await self._registry.list_agents()
        now = int(time.time())
        blocked = []
        for agent in agents:
            if agent.status == "blocked":
                blocked_duration = now - agent.last_heartbeat
                if blocked_duration > self._blocked_threshold:
                    blocked.append(agent)
        return blocked
