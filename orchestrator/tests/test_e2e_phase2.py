"""Phase 2 integration test: bootstrap creates tickets, runner can read them."""
import json
import pytest
from unittest.mock import AsyncMock

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.models import InboxMessage, Ticket, TicketStatus
from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.prompts.manager import build_manager_prompt
from synapse_os.prompts.worker import build_worker_prompt
from synapse_os.runner import Runner

pytestmark = pytest.mark.asyncio


async def test_bootstrap_to_runner_flow(redis):
    """Simulate: bootstrap creates tickets → runner picks them up."""
    brain = Brain(redis)
    config = Config()

    # 1. Simulate what the bootstrap agent would do (create tickets)
    import synapse_os.tools.server as srv
    srv._redis = redis
    srv._brain = brain
    srv._agent_id = "bootstrap"
    srv._agent_role = "bootstrap"

    from synapse_os.tools.ticket_tools import _create_ticket
    result1 = await _create_ticket("Build auth", "Implement login/signup", "backend", 1)
    result2 = await _create_ticket("Add tests", "Write unit tests for auth", "backend", 2)

    t1 = json.loads(result1)
    t2 = json.loads(result2)
    assert t1["status"] == "created"
    assert t2["status"] == "created"

    # 2. Verify tickets are in the work queue
    runner = Runner(
        config=config, project_dir="/tmp/test", redis=redis, brain=brain,
        domain="backend", num_workers=1,
    )
    pending = await runner.get_pending_tickets()
    assert len(pending) == 2

    # 3. Simulate worker claiming a ticket
    ticket_id = await brain.claim_ticket(timeout=1)
    assert ticket_id is not None
    await brain.register_claim(ticket_id, "w-backend-0")

    # 4. Simulate worker completion → manager notification
    srv._agent_role = "worker"
    srv._manager_id = "mgr-backend"
    from synapse_os.tools.communication_tools import _notify_manager
    await _notify_manager(ticket_id, pr_url="https://github.com/test/pull/1")

    msg_raw = await brain.pop_manager_inbox("mgr-backend", timeout=1)
    msg = InboxMessage.from_json(msg_raw)
    assert msg.msg_type == "completion"
    assert msg.payload["pr_url"] == "https://github.com/test/pull/1"


async def test_prompts_are_well_formed():
    """Verify all prompt generators produce non-empty, useful prompts."""
    bp = build_bootstrap_prompt("/tmp/repo", "Build a todo app")
    assert len(bp) > 200
    assert "create_ticket" in bp

    mp = build_manager_prompt("frontend", "React app")
    assert len(mp) > 200
    assert "assign_ticket" in mp

    wp = build_worker_prompt("t-1", "Build login", "Create login form", "frontend")
    assert len(wp) > 200
    assert "notify_manager" in wp
