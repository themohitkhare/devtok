import asyncio
import pytest
from unittest.mock import AsyncMock
from synapse_os.stdout_router import StdoutRouter

pytestmark = pytest.mark.asyncio


async def test_routes_stdout_lines():
    stdb = AsyncMock()
    router = StdoutRouter(stdb_client=stdb, project_id=1, stdb_agent_id=1)

    lines = [b"line 1\n", b"line 2\n", b""]
    read_calls = iter(lines)

    class FakeStream:
        async def readline(self):
            return next(read_calls)

    await router.route_stream(FakeStream(), agent_id="w-1")

    assert stdb.post_action_card.call_count == 2
    call_args = stdb.post_action_card.call_args_list[0]
    assert call_args.kwargs["visual_type"] == "TerminalOutput"
    assert "line 1" in call_args.kwargs["content"]


async def test_batches_rapid_lines():
    stdb = AsyncMock()
    router = StdoutRouter(stdb_client=stdb, project_id=1, stdb_agent_id=1, batch_interval=0.1)

    lines = [b"line 1\n", b"line 2\n", b"line 3\n", b""]
    read_calls = iter(lines)

    class FakeStream:
        async def readline(self):
            return next(read_calls)

    await router.route_stream(FakeStream(), agent_id="w-1", batch=True)

    assert stdb.post_action_card.call_count == 1
    content = stdb.post_action_card.call_args.kwargs["content"]
    assert "line 1" in content
    assert "line 3" in content


async def test_empty_stream_no_cards():
    stdb = AsyncMock()
    router = StdoutRouter(stdb_client=stdb, project_id=1, stdb_agent_id=1)

    class FakeStream:
        async def readline(self):
            return b""

    await router.route_stream(FakeStream(), agent_id="w-1")
    stdb.post_action_card.assert_not_called()
