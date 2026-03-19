from __future__ import annotations

import time

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.models import AgentRole, AgentState


class AgentRegistry:
    _AGENTS_SET_KEY = "registered_agents"

    def __init__(self, brain: Brain, redis: Redis) -> None:
        self._brain = brain
        self._r = redis

    async def register(self, agent_id: str, role: AgentRole, domain: str, manager_id: str, process_pid: int) -> None:
        state = AgentState(
            agent_id=agent_id, role=role, domain=domain, status="idle",
            current_ticket=None, manager_id=manager_id, process_pid=process_pid,
            last_heartbeat=int(time.time()),
        )
        await self._r.hset(self._brain.key_agent_state(agent_id), mapping=state.to_dict())
        await self._r.sadd(self._AGENTS_SET_KEY, agent_id)

    async def deregister(self, agent_id: str) -> None:
        await self._r.delete(self._brain.key_agent_state(agent_id))
        await self._r.srem(self._AGENTS_SET_KEY, agent_id)

    async def heartbeat(self, agent_id: str) -> None:
        await self._r.hset(self._brain.key_agent_state(agent_id), "last_heartbeat", str(int(time.time())))

    async def update_status(self, agent_id: str, status: str, current_ticket: str | None = None) -> None:
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
