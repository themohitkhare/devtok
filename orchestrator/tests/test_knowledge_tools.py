import pytest
import pytest_asyncio
import json
from synapse_os.brain import Brain
from synapse_os.tools import knowledge_tools

pytestmark = pytest.mark.asyncio


@pytest_asyncio.fixture(autouse=True)
async def setup_server_globals(redis):
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = Brain(redis)
    srv._agent_id = "test-mgr"
    srv._agent_role = "manager"
    srv._manager_id = ""
    yield


async def test_write_and_read_knowledge(redis):
    result = await knowledge_tools._write_knowledge(
        domain="backend", key="api_schema", value='{"users": "/api/v1/users"}', expected_version=0,
    )
    parsed = json.loads(result)
    assert parsed["status"] == "written"
    assert parsed["new_version"] == 1

    result = await knowledge_tools._read_knowledge(domain="backend", key="api_schema")
    parsed = json.loads(result)
    assert parsed["value"] == '{"users": "/api/v1/users"}'
    assert parsed["version"] == 1


async def test_write_version_conflict(redis):
    await knowledge_tools._write_knowledge("backend", "schema", "v1", 0)
    result = await knowledge_tools._write_knowledge("backend", "schema", "v2", 0)
    parsed = json.loads(result)
    assert parsed["status"] == "conflict"


async def test_read_missing_key(redis):
    result = await knowledge_tools._read_knowledge("backend", "nonexistent")
    parsed = json.loads(result)
    assert parsed["value"] is None
