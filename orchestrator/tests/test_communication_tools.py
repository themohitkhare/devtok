import pytest
import pytest_asyncio
import json
from synapse_os.brain import Brain
from synapse_os.tools import communication_tools

pytestmark = pytest.mark.asyncio


@pytest_asyncio.fixture(autouse=True)
async def setup_server_globals(redis):
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = Brain(redis)
    srv._agent_id = "mgr-1"
    srv._agent_role = "manager"
    srv._manager_id = ""
    yield


async def test_send_agent_message(redis):
    result = await communication_tools._send_agent_message("w-1", "Please prioritize the auth ticket")
    parsed = json.loads(result)
    assert parsed["status"] == "sent"

    brain = Brain(redis)
    msg = await brain.pop_inbox("w-1", timeout=1)
    assert msg is not None
    assert "prioritize" in json.loads(msg)["payload"]["text"]


async def test_post_status_card(redis):
    import synapse_os.tools.server as srv
    from unittest.mock import AsyncMock
    srv._stdb_client = AsyncMock()
    result = await communication_tools._post_status_card("Build phase complete", priority=1)
    parsed = json.loads(result)
    assert parsed["status"] == "posted"


async def test_worker_sends_completion(redis):
    import synapse_os.tools.server as srv
    srv._agent_role = "worker"
    srv._manager_id = "mgr-1"
    result = await communication_tools._notify_manager(
        ticket_id="t-1", pr_url="https://github.com/org/repo/pull/1",
    )
    parsed = json.loads(result)
    assert parsed["status"] == "notified"

    brain = Brain(redis)
    msg = await brain.pop_manager_inbox("mgr-1", timeout=1)
    assert msg is not None
    data = json.loads(msg)
    assert data["msg_type"] == "completion"
    assert data["payload"]["pr_url"] == "https://github.com/org/repo/pull/1"
