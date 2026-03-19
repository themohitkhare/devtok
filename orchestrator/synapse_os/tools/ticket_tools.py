"""MCP tools for ticket management."""
from __future__ import annotations

import json
import uuid

from synapse_os.models import InboxMessage, Ticket, TicketStatus
from synapse_os.tools.server import get_agent_id, get_agent_role, get_brain, get_redis, mcp


def _require_role(*allowed: str) -> str | None:
    role = get_agent_role()
    if role not in allowed:
        return json.dumps({"error": f"Not authorized. Role '{role}' cannot use this tool. Allowed: {allowed}"})
    return None


@mcp.tool()
async def create_ticket(title: str, description: str, domain: str, priority: int, assignee: str = "") -> str:
    """Create a new ticket and add it to the work queue.

    Args:
        title: Short ticket title
        description: Detailed description of the work
        domain: Domain this ticket belongs to (frontend, backend, devops, etc.)
        priority: Priority level (1=highest, 5=lowest)
        assignee: Optional agent ID to directly assign to
    """
    return await _create_ticket(title, description, domain, priority, assignee)


async def _create_ticket(title: str, description: str, domain: str, priority: int, assignee: str = "") -> str:
    denied = _require_role("manager", "bootstrap")
    if denied:
        return denied

    brain = get_brain()
    redis = get_redis()
    ticket_id = f"t-{uuid.uuid4().hex[:8]}"

    ticket = Ticket(
        ticket_id=ticket_id, title=title, description=description,
        domain=domain, priority=priority, status=TicketStatus.PENDING,
        assignee=assignee or None,
    )
    await redis.hset(brain.key_ticket(ticket_id), mapping=ticket.to_dict())

    if assignee:
        msg = InboxMessage(
            msg_type="ticket_assignment",
            payload={"ticket_id": ticket_id, "title": title, "description": description},
            sender=get_agent_id(),
        )
        await brain.push_inbox(assignee, msg.to_json())
    else:
        await brain.enqueue_ticket(ticket_id)

    return json.dumps({"status": "created", "ticket_id": ticket_id})


@mcp.tool()
async def assign_ticket(ticket_id: str, agent_id: str) -> str:
    """Assign a ticket directly to a specific agent.

    Args:
        ticket_id: The ticket ID to assign
        agent_id: The worker agent ID to assign to
    """
    return await _assign_ticket(ticket_id, agent_id)


async def _assign_ticket(ticket_id: str, agent_id: str) -> str:
    denied = _require_role("manager")
    if denied:
        return denied

    brain = get_brain()
    redis = get_redis()
    await redis.hset(brain.key_ticket(ticket_id), mapping={"assignee": agent_id, "status": "pending"})

    title = await redis.hget(brain.key_ticket(ticket_id), "title") or ""
    desc = await redis.hget(brain.key_ticket(ticket_id), "description") or ""
    msg = InboxMessage(
        msg_type="ticket_assignment",
        payload={"ticket_id": ticket_id, "title": title, "description": desc},
        sender=get_agent_id(),
    )
    await brain.push_inbox(agent_id, msg.to_json())

    return json.dumps({"status": "assigned", "ticket_id": ticket_id, "agent_id": agent_id})


@mcp.tool()
async def update_ticket(ticket_id: str, status: str, notes: str = "") -> str:
    """Update a ticket's status and notes. Manager only.

    Args:
        ticket_id: The ticket ID
        status: New status (pending, in_progress, review_pending, blocked, completed)
        notes: Optional notes to add
    """
    return await _update_ticket(ticket_id, status, notes)


async def _update_ticket(ticket_id: str, status: str, notes: str = "") -> str:
    denied = _require_role("manager")
    if denied:
        return denied

    redis = get_redis()
    brain = get_brain()
    mapping: dict[str, str] = {"status": status}
    if notes:
        mapping["notes"] = notes
    await redis.hset(brain.key_ticket(ticket_id), mapping=mapping)
    return json.dumps({"status": "updated", "ticket_id": ticket_id, "new_status": status})


@mcp.tool()
async def update_ticket_status(ticket_id: str, status: str) -> str:
    """Update ticket status. Workers may only set: in_progress, review_pending, blocked.

    Args:
        ticket_id: The ticket ID
        status: New status
    """
    return await _update_ticket_status(ticket_id, status)


async def _update_ticket_status(ticket_id: str, status: str) -> str:
    role = get_agent_role()
    allowed_worker_statuses = {"in_progress", "review_pending", "blocked"}

    if role == "worker" and status not in allowed_worker_statuses:
        return json.dumps({"error": f"Workers can only set status to: {allowed_worker_statuses}"})

    redis = get_redis()
    brain = get_brain()
    await redis.hset(brain.key_ticket(ticket_id), "status", status)
    return json.dumps({"status": "updated", "ticket_id": ticket_id, "new_status": status})
