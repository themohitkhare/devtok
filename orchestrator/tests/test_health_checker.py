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

    pm = MagicMock()
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
    await redis.hset(brain.key_agent_state("w-1"), "last_heartbeat", "1000")

    blocked = await checker.check_blocked()
    assert "w-1" in [a.agent_id for a in blocked]
