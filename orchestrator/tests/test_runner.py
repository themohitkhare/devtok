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
    runner = Runner(config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain, domain="frontend", num_workers=2)
    assert runner._domain == "frontend"
    assert runner._num_workers == 2


async def test_runner_creates_manager_and_workers(redis):
    brain = Brain(redis)
    runner = Runner(config=Config(), project_dir="/tmp/myrepo", redis=redis, brain=brain, domain="frontend", num_workers=2)
    mock_proc = MagicMock()
    mock_proc.agent_id = "test"
    mock_proc.pid = 100
    mock_proc.process = MagicMock()
    mock_proc.process.wait = AsyncMock(return_value=0)

    with patch.object(runner._spawner, 'spawn_agent', new_callable=AsyncMock, return_value=mock_proc):
        await runner.start()
        assert runner._spawner.spawn_agent.call_count == 3  # 1 manager + 2 workers


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
