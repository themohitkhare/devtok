import json
import pytest
from unittest.mock import AsyncMock
from synapse_os.brain import Brain
from synapse_os.feedback_watcher import FeedbackWatcher

pytestmark = pytest.mark.asyncio


async def test_forwards_escalation_response(redis):
    brain = Brain(redis)
    stdb = AsyncMock()

    await redis.set(brain.key_escalation_card("10"), "mgr-1")

    stdb.query_feedback_since.return_value = [
        [1, 10, "approve", "", 2000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    await watcher.poll_once(since_micros=1000000)

    msg_raw = await brain.pop_manager_inbox("mgr-1", timeout=1)
    assert msg_raw is not None
    msg = json.loads(msg_raw)
    assert msg["msg_type"] == "escalation_response"
    assert msg["payload"]["approved"] is True
    assert msg["payload"]["card_id"] == 10


async def test_forwards_reject_with_comment(redis):
    brain = Brain(redis)
    stdb = AsyncMock()
    await redis.set(brain.key_escalation_card("11"), "mgr-2")

    stdb.query_feedback_since.return_value = [
        [2, 11, "reject", "needs more testing", 2000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    await watcher.poll_once(since_micros=1000000)

    msg_raw = await brain.pop_manager_inbox("mgr-2", timeout=1)
    msg = json.loads(msg_raw)
    assert msg["payload"]["approved"] is False
    assert msg["payload"]["comment"] == "needs more testing"


async def test_ignores_non_escalation_feedback(redis):
    brain = Brain(redis)
    stdb = AsyncMock()

    stdb.query_feedback_since.return_value = [
        [3, 99, "comment", "looks good", 2000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    await watcher.poll_once(since_micros=1000000)

    msg = await redis.blpop("manager_inbox:mgr-1", timeout=0.1)
    assert msg is None


async def test_tracks_high_water_mark(redis):
    brain = Brain(redis)
    stdb = AsyncMock()
    await redis.set(brain.key_escalation_card("10"), "mgr-1")

    stdb.query_feedback_since.return_value = [
        [1, 10, "approve", "", 5000000],
    ]

    watcher = FeedbackWatcher(brain=brain, redis=redis, stdb_client=stdb)
    new_hwm = await watcher.poll_once(since_micros=1000000)
    assert new_hwm == 5000000
