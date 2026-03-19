import pytest
import json
from synapse_os.brain import Brain

pytestmark = pytest.mark.asyncio


async def test_key_helpers():
    assert Brain.key_agent_inbox("a1") == "agent_inbox:a1"
    assert Brain.key_manager_inbox("m1") == "manager_inbox:m1"
    assert Brain.key_agent_state("a1") == "agent_state:a1"
    assert Brain.key_ticket("t1") == "tickets:t1"
    assert Brain.key_standup_responses("m1") == "standup_responses:m1"
    assert Brain.key_knowledge("frontend", "api") == "knowledge_base:frontend:api"
    assert Brain.key_knowledge_ver("api") == "knowledge_base_ver:api"
    assert Brain.key_meeting("meet-1") == "meeting:meet-1"


async def test_push_and_pop_inbox(redis):
    brain = Brain(redis)
    await brain.push_inbox("a1", '{"msg_type":"test","payload":{},"sender":"s"}')
    msg = await brain.pop_inbox("a1", timeout=1)
    assert msg is not None
    assert json.loads(msg)["msg_type"] == "test"


async def test_pop_inbox_empty_returns_none(redis):
    brain = Brain(redis)
    msg = await brain.pop_inbox("a1", timeout=1)
    assert msg is None


async def test_push_and_pop_manager_inbox(redis):
    brain = Brain(redis)
    await brain.push_manager_inbox("m1", '{"msg_type":"completion","payload":{},"sender":"w1"}')
    msg = await brain.pop_manager_inbox("m1", timeout=1)
    assert json.loads(msg)["msg_type"] == "completion"


async def test_enqueue_and_claim_ticket(redis):
    brain = Brain(redis)
    await brain.enqueue_ticket("t-1")
    ticket_id = await brain.claim_ticket(timeout=1)
    assert ticket_id == "t-1"


async def test_register_claim(redis):
    brain = Brain(redis)
    await brain.enqueue_ticket("t-1")
    await redis.hset(brain.key_ticket("t-1"), mapping={"status": "pending", "assignee": ""})
    ticket_id = await brain.claim_ticket(timeout=1)
    result = await brain.register_claim(ticket_id, "agent-1")
    assert result is True
    assert await redis.hget("in_progress", "t-1") == "agent-1"
    assert await redis.hget(brain.key_ticket("t-1"), "status") == "in_progress"
    assert await redis.hget(brain.key_ticket("t-1"), "assignee") == "agent-1"


async def test_claim_empty_queue_returns_none(redis):
    brain = Brain(redis)
    ticket_id = await brain.claim_ticket(timeout=1)
    assert ticket_id is None


async def test_knowledge_write_new_key(redis):
    brain = Brain(redis)
    ok = await brain.write_knowledge("frontend", "api_shape", '{"endpoint": "/users"}', expected_version=0)
    assert ok is True
    val = await redis.get(brain.key_knowledge("frontend", "api_shape"))
    assert val == '{"endpoint": "/users"}'
    ver = await redis.get(brain.key_knowledge_ver("api_shape"))
    assert ver == "1"


async def test_knowledge_write_version_conflict(redis):
    brain = Brain(redis)
    await brain.write_knowledge("frontend", "api_shape", "v1", expected_version=0)
    ok = await brain.write_knowledge("frontend", "api_shape", "v2", expected_version=0)
    assert ok is False


async def test_knowledge_read(redis):
    brain = Brain(redis)
    await brain.write_knowledge("backend", "schema", "users_table", expected_version=0)
    value, version = await brain.read_knowledge("backend", "schema")
    assert value == "users_table"
    assert version == 1
