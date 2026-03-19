import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.models import AgentRole
from synapse_os.runner import Runner

pytestmark = pytest.mark.asyncio


async def test_runner_init(redis):
    brain = Brain(redis)
    runner = Runner(config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain, domain="frontend", num_workers=2)
    assert runner._domain == "frontend"
    assert runner._num_workers == 2


async def test_runner_start_registers_agents(redis):
    brain = Brain(redis)
    runner = Runner(config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain, domain="frontend", num_workers=2)
    await runner.start()
    # start() now just registers agents, doesn't spawn
    from synapse_os.registry import AgentRegistry
    registry = AgentRegistry(brain, redis)
    mgr = await registry.get_state("mgr-frontend")
    assert mgr is not None
    assert mgr.role == AgentRole.MANAGER
    w0 = await registry.get_state("w-frontend-0")
    assert w0 is not None
    assert w0.role == AgentRole.WORKER
    assert len(runner._worker_ids) == 2


async def test_runner_assigns_pending_tickets(redis):
    brain = Brain(redis)
    runner = Runner(config=Config(), project_dir="/tmp/test", redis=redis, brain=brain, domain="frontend", num_workers=1)
    await redis.hset(brain.key_ticket("t-1"), mapping={
        "ticket_id": "t-1", "title": "Test ticket", "description": "Do something",
        "domain": "frontend", "priority": "1", "status": "pending", "assignee": "", "notes": "",
    })
    await brain.enqueue_ticket("t-1")
    pending = await runner.get_pending_tickets()
    assert len(pending) >= 1
