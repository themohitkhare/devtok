import json
from synapse_os.models import (
    AgentState,
    AgentRole,
    InboxMessage,
    Ticket,
    TicketStatus,
    StandupResponse,
)


def test_agent_state_to_dict():
    state = AgentState(
        agent_id="agent-1",
        role=AgentRole.WORKER,
        domain="frontend",
        status="idle",
        current_ticket=None,
        manager_id="mgr-1",
        process_pid=12345,
    )
    d = state.to_dict()
    assert d["agent_id"] == "agent-1"
    assert d["role"] == "worker"
    assert d["status"] == "idle"
    assert d["current_ticket"] == ""


def test_agent_state_from_dict():
    d = {
        "agent_id": "agent-1",
        "role": "worker",
        "domain": "frontend",
        "status": "idle",
        "current_ticket": "",
        "manager_id": "mgr-1",
        "process_pid": "12345",
        "last_heartbeat": "1000000",
    }
    state = AgentState.from_dict(d)
    assert state.agent_id == "agent-1"
    assert state.role == AgentRole.WORKER
    assert state.current_ticket is None
    assert state.process_pid == 12345


def test_ticket_to_dict():
    t = Ticket(
        ticket_id="t-1",
        title="Fix login",
        description="Auth is broken",
        domain="backend",
        priority=1,
        status=TicketStatus.PENDING,
    )
    d = t.to_dict()
    assert d["status"] == "pending"
    assert d["assignee"] == ""


def test_inbox_message_roundtrip():
    msg = InboxMessage(
        msg_type="ticket_assignment",
        payload={"ticket_id": "t-1"},
        sender="mgr-1",
    )
    serialized = msg.to_json()
    restored = InboxMessage.from_json(serialized)
    assert restored.msg_type == "ticket_assignment"
    assert restored.payload["ticket_id"] == "t-1"


def test_standup_response_to_json():
    sr = StandupResponse(
        agent_id="agent-1",
        did="Implemented auth endpoint",
        doing="Writing tests",
        blocked=None,
    )
    parsed = json.loads(sr.to_json())
    assert parsed["agent_id"] == "agent-1"
    assert parsed["blocked"] is None
