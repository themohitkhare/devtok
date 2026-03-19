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
        agent_id="w-1", role=AgentRole.WORKER, domain="frontend", manager_id="m-1", process_pid=9999,
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
    await redis.hset(brain.key_agent_state("w-1"), "last_heartbeat", "1000")
    stale = await registry.get_stale_agents(timeout_seconds=60)
    assert "w-1" in [s.agent_id for s in stale]
