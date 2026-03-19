"""MCP tools for inter-agent communication."""
from __future__ import annotations

import json

from synapse_os.models import InboxMessage
from synapse_os.tools.server import get_agent_id, get_agent_role, get_brain, get_manager_id, mcp


@mcp.tool()
async def send_agent_message(agent_id: str, message: str) -> str:
    """Send a message to another agent's inbox.

    Args:
        agent_id: Target agent ID
        message: Message text
    """
    return await _send_agent_message(agent_id, message)


async def _send_agent_message(agent_id: str, message: str) -> str:
    brain = get_brain()
    msg = InboxMessage(
        msg_type="direct_message",
        payload={"text": message},
        sender=get_agent_id(),
    )
    await brain.push_inbox(agent_id, msg.to_json())
    return json.dumps({"status": "sent", "to": agent_id})


@mcp.tool()
async def post_status_card(content: str, priority: int = 0) -> str:
    """Post a status update card to Synapse feed for human visibility.

    Args:
        content: Status update text
        priority: Priority (0=normal, 1=high)
    """
    return await _post_status_card(content, priority)


async def _post_status_card(content: str, priority: int = 0) -> str:
    from synapse_os.tools.server import _stdb_client
    if _stdb_client:
        try:
            await _stdb_client.post_action_card(
                agent_id=1, project_id=1, visual_type="StatusUpdate",
                content=content, task_summary=f"Status from {get_agent_id()}", priority=priority,
            )
        except Exception:
            pass
    return json.dumps({"status": "posted", "content": content[:50]})


@mcp.tool()
async def notify_manager(ticket_id: str, pr_url: str = "", message: str = "") -> str:
    """Notify your manager that work is complete or needs attention.

    Args:
        ticket_id: The ticket this is about
        pr_url: URL of the pull request (if work is complete)
        message: Optional message
    """
    return await _notify_manager(ticket_id, pr_url, message)


async def _notify_manager(ticket_id: str, pr_url: str = "", message: str = "") -> str:
    brain = get_brain()
    manager_id = get_manager_id()
    if not manager_id:
        return json.dumps({"error": "No manager_id configured"})

    msg = InboxMessage(
        msg_type="completion" if pr_url else "status_update",
        payload={"ticket_id": ticket_id, "pr_url": pr_url, "message": message},
        sender=get_agent_id(),
    )
    await brain.push_manager_inbox(manager_id, msg.to_json())
    return json.dumps({"status": "notified", "manager_id": manager_id})
