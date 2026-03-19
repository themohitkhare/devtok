"""Integration test: exercises the full flow with fakeredis and mocked SpacetimeDB."""
import asyncio
import json
import pytest
from unittest.mock import AsyncMock

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.daemon import Daemon
from synapse_os.models import AgentRole, InboxMessage, Ticket, TicketStatus

pytestmark = pytest.mark.asyncio


async def test_full_lifecycle(redis):
    """Register agent → enqueue ticket → claim → complete → feedback forward."""
    cfg = Config()
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)
    brain = daemon._brain
    registry = daemon._registry

    # 1. Register a manager and a worker
    await registry.register("m-1", AgentRole.MANAGER, "frontend", "", 100)
    await registry.register("w-1", AgentRole.WORKER, "frontend", "m-1", 200)

    # 2. Create a ticket and enqueue it
    ticket = Ticket(
        ticket_id="t-1", title="Build login page",
        description="Create login form with email/password",
        domain="frontend", priority=1, status=TicketStatus.PENDING,
    )
    await redis.hset(brain.key_ticket("t-1"), mapping=ticket.to_dict())
    await brain.enqueue_ticket("t-1")

    # 3. Worker claims the ticket
    claimed = await brain.claim_ticket(timeout=1)
    assert claimed == "t-1"
    result = await brain.register_claim("t-1", "w-1")
    assert result is True

    # Verify state
    assert await redis.hget("in_progress", "t-1") == "w-1"
    assert await redis.hget(brain.key_ticket("t-1"), "status") == "in_progress"

    # 4. Worker completes and notifies manager
    completion = InboxMessage(
        msg_type="completion",
        payload={"ticket_id": "t-1", "pr_url": "https://github.com/org/repo/pull/1"},
        sender="w-1",
    )
    await brain.push_manager_inbox("m-1", completion.to_json())

    # 5. Manager reads completion
    msg_raw = await brain.pop_manager_inbox("m-1", timeout=1)
    msg = InboxMessage.from_json(msg_raw)
    assert msg.msg_type == "completion"
    assert msg.payload["pr_url"] == "https://github.com/org/repo/pull/1"

    # 6. Simulate escalation feedback forwarding
    await redis.set(brain.key_escalation_card("42"), "m-1")
    stdb.query_feedback_since.return_value = [
        [1, 42, "approve", "", 9999999],
    ]
    new_hwm = await daemon._feedback.poll_once(0)
    assert new_hwm == 9999999

    esc_msg_raw = await brain.pop_manager_inbox("m-1", timeout=1)
    esc_msg = InboxMessage.from_json(esc_msg_raw)
    assert esc_msg.msg_type == "escalation_response"
    assert esc_msg.payload["approved"] is True
