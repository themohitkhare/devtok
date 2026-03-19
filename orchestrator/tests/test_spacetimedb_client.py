import json
import pytest
import httpx
from synapse_os.config import Config
from synapse_os.spacetimedb_client import SpacetimeDBClient

pytestmark = pytest.mark.asyncio


class MockTransport(httpx.AsyncBaseTransport):
    def __init__(self):
        self.requests: list[httpx.Request] = []
        self.responses: list[httpx.Response] = []

    def add_response(self, status_code=200, json_data=None, text=None):
        if json_data is not None:
            self.responses.append(httpx.Response(status_code, json=json_data))
        else:
            self.responses.append(httpx.Response(status_code, text=text or ""))

    async def handle_async_request(self, request):
        self.requests.append(request)
        return self.responses.pop(0) if self.responses else httpx.Response(200)


@pytest.fixture
def mock_transport():
    return MockTransport()


@pytest.fixture
def stdb_client(mock_transport):
    cfg = Config()
    client = SpacetimeDBClient(cfg, transport=mock_transport)
    return client


async def test_call_reducer(stdb_client, mock_transport):
    mock_transport.add_response(status_code=200, text="")
    await stdb_client.call_reducer("insert_action_card", ["agent-1", "proj-1", "StatusUpdate", "content", "summary", 1])
    req = mock_transport.requests[0]
    assert "/call/insert_action_card" in str(req.url)
    body = json.loads(req.content)
    assert body[0] == "agent-1"


async def test_query_sql(stdb_client, mock_transport):
    mock_transport.add_response(json_data=[{"rows": [["row1-col1", "row1-col2"]]}])
    rows = await stdb_client.query_sql("SELECT * FROM feedback")
    assert len(rows) == 1
    assert rows[0][0] == "row1-col1"


async def test_post_action_card(stdb_client, mock_transport):
    mock_transport.add_response(status_code=200, text="")
    await stdb_client.post_action_card(
        agent_id=1, project_id=1, visual_type="StatusUpdate",
        content="test content", task_summary="test summary", priority=1,
    )
    req = mock_transport.requests[0]
    assert "/call/insert_action_card" in str(req.url)


async def test_query_feedback_since(stdb_client, mock_transport):
    mock_transport.add_response(json_data=[{"rows": [
        [1, 10, "approve", "", 1000000],
        [2, 11, "reject", "bad code", 1000001],
    ]}])
    rows = await stdb_client.query_feedback_since(999999)
    assert len(rows) == 2
    assert rows[0][2] == "approve"
