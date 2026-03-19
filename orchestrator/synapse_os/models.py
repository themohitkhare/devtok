from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class AgentRole(Enum):
    WORKER = "worker"
    MANAGER = "manager"
    BOOTSTRAP = "bootstrap"


class TicketStatus(Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    REVIEW_PENDING = "review_pending"
    BLOCKED = "blocked"
    COMPLETED = "completed"


@dataclass
class AgentState:
    agent_id: str
    role: AgentRole
    domain: str
    status: str
    current_ticket: str | None
    manager_id: str
    process_pid: int
    last_heartbeat: int = 0

    def to_dict(self) -> dict[str, str]:
        return {
            "agent_id": self.agent_id,
            "role": self.role.value,
            "domain": self.domain,
            "status": self.status,
            "current_ticket": self.current_ticket or "",
            "manager_id": self.manager_id,
            "process_pid": str(self.process_pid),
            "last_heartbeat": str(self.last_heartbeat),
        }

    @classmethod
    def from_dict(cls, d: dict[str, str]) -> AgentState:
        return cls(
            agent_id=d["agent_id"],
            role=AgentRole(d["role"]),
            domain=d["domain"],
            status=d["status"],
            current_ticket=d["current_ticket"] or None,
            manager_id=d["manager_id"],
            process_pid=int(d["process_pid"]),
            last_heartbeat=int(d.get("last_heartbeat", "0")),
        )


@dataclass
class Ticket:
    ticket_id: str
    title: str
    description: str
    domain: str
    priority: int
    status: TicketStatus
    assignee: str | None = None
    notes: str = ""

    def to_dict(self) -> dict[str, str]:
        return {
            "ticket_id": self.ticket_id,
            "title": self.title,
            "description": self.description,
            "domain": self.domain,
            "priority": str(self.priority),
            "status": self.status.value,
            "assignee": self.assignee or "",
            "notes": self.notes,
        }

    @classmethod
    def from_dict(cls, d: dict[str, str]) -> Ticket:
        return cls(
            ticket_id=d["ticket_id"],
            title=d["title"],
            description=d["description"],
            domain=d["domain"],
            priority=int(d["priority"]),
            status=TicketStatus(d["status"]),
            assignee=d["assignee"] or None,
            notes=d.get("notes", ""),
        )


@dataclass
class InboxMessage:
    msg_type: str
    payload: dict[str, Any]
    sender: str
    timestamp: int = 0

    def to_json(self) -> str:
        return json.dumps({
            "msg_type": self.msg_type,
            "payload": self.payload,
            "sender": self.sender,
            "timestamp": self.timestamp,
        })

    @classmethod
    def from_json(cls, raw: str) -> InboxMessage:
        d = json.loads(raw)
        return cls(
            msg_type=d["msg_type"],
            payload=d["payload"],
            sender=d["sender"],
            timestamp=d.get("timestamp", 0),
        )


@dataclass
class StandupResponse:
    agent_id: str
    did: str
    doing: str
    blocked: str | None

    def to_json(self) -> str:
        return json.dumps({
            "agent_id": self.agent_id,
            "did": self.did,
            "doing": self.doing,
            "blocked": self.blocked,
        })

    @classmethod
    def from_json(cls, raw: str) -> StandupResponse:
        d = json.loads(raw)
        return cls(
            agent_id=d["agent_id"],
            did=d["did"],
            doing=d["doing"],
            blocked=d["blocked"],
        )
