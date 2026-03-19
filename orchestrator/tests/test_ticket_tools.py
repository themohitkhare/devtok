import pytest
import pytest_asyncio
import json
from synapse_os.brain import Brain
from synapse_os.tools import ticket_tools

pytestmark = pytest.mark.asyncio


@pytest_asyncio.fixture(autouse=True)
async def setup_server_globals(redis):
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = Brain(redis)
    srv._agent_id = "test-agent"
    srv._agent_role = "manager"
    srv._manager_id = ""
    yield


async def test_create_ticket(redis):
    result = await ticket_tools._create_ticket(
        title="Build auth", description="Add login/signup", domain="backend", priority=1,
    )
    parsed = json.loads(result)
    assert parsed["status"] == "created"
    ticket_id = parsed["ticket_id"]
    brain = Brain(redis)
    data = await redis.hgetall(brain.key_ticket(ticket_id))
    assert data["title"] == "Build auth"
    assert data["status"] == "pending"


async def test_assign_ticket(redis):
    brain = Brain(redis)
    await redis.hset(brain.key_ticket("t-1"), mapping={
        "ticket_id": "t-1", "title": "Test", "description": "Test",
        "domain": "backend", "priority": "1", "status": "pending", "assignee": "", "notes": "",
    })
    result = await ticket_tools._assign_ticket(ticket_id="t-1", agent_id="w-1")
    parsed = json.loads(result)
    assert parsed["status"] == "assigned"
    msg = await brain.pop_inbox("w-1", timeout=1)
    assert msg is not None
    assert "t-1" in msg


async def test_update_ticket_status(redis):
    brain = Brain(redis)
    await redis.hset(brain.key_ticket("t-1"), mapping={
        "ticket_id": "t-1", "title": "Test", "description": "Test",
        "domain": "backend", "priority": "1", "status": "pending", "assignee": "", "notes": "",
    })
    result = await ticket_tools._update_ticket_status(ticket_id="t-1", status="in_progress")
    parsed = json.loads(result)
    assert parsed["status"] == "updated"
    assert await redis.hget(brain.key_ticket("t-1"), "status") == "in_progress"


async def test_worker_cannot_use_create_ticket(redis):
    import synapse_os.tools.server as srv
    srv._agent_role = "worker"
    result = await ticket_tools._create_ticket(
        title="Nope", description="Nope", domain="backend", priority=1,
    )
    assert "not authorized" in result.lower()
