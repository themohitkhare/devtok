"""CLI bridge for Synapse OS tools.

Allows Claude Code agents to call brain tools via Bash instead of MCP.
Each subcommand wraps one MCP tool function.

Usage:
    synapse-tool create-ticket --title "..." --description "..." --domain "..." --priority 1
    synapse-tool read-kb --domain "..." --key "..."
    synapse-tool write-kb --domain "..." --key "..." --value "..."
    synapse-tool notify-manager --ticket-id "..." --pr-url "..."
    synapse-tool status-card --content "..."
    synapse-tool assign-ticket --ticket-id "..." --agent-id "..."
    synapse-tool update-ticket-status --ticket-id "..." --status "..."
"""
from __future__ import annotations

import asyncio
import os
import sys

import click

# Ensure orchestrator package is importable
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

import redis.asyncio as aioredis
from synapse_os.brain import Brain


def _get_globals():
    """Set up server globals from environment."""
    import synapse_os.tools.server as srv
    redis_url = os.environ.get("SYNAPSE_REDIS_URL", "redis://localhost:6379/0")

    async def init():
        srv._redis = aioredis.from_url(redis_url, decode_responses=True)
        srv._brain = Brain(srv._redis)
        srv._agent_id = os.environ.get("SYNAPSE_AGENT_ID", "unknown")
        srv._agent_role = os.environ.get("SYNAPSE_AGENT_ROLE", "bootstrap")
        srv._manager_id = os.environ.get("SYNAPSE_MANAGER_ID", "")

    asyncio.run(init())


@click.group()
def tool_cli():
    """Synapse OS tool bridge — call brain tools from the command line."""
    _get_globals()


@tool_cli.command("create-ticket")
@click.option("--title", required=True)
@click.option("--description", required=True)
@click.option("--domain", required=True)
@click.option("--priority", type=int, default=3)
@click.option("--assignee", default="")
def create_ticket(title, description, domain, priority, assignee):
    """Create a ticket and add to work queue."""
    from synapse_os.tools.ticket_tools import _create_ticket
    result = asyncio.run(_create_ticket(title, description, domain, priority, assignee))
    click.echo(result)


@tool_cli.command("assign-ticket")
@click.option("--ticket-id", required=True)
@click.option("--agent-id", required=True)
def assign_ticket(ticket_id, agent_id):
    """Assign a ticket to a worker."""
    from synapse_os.tools.ticket_tools import _assign_ticket
    result = asyncio.run(_assign_ticket(ticket_id, agent_id))
    click.echo(result)


@tool_cli.command("update-ticket-status")
@click.option("--ticket-id", required=True)
@click.option("--status", required=True)
def update_ticket_status(ticket_id, status):
    """Update ticket status."""
    from synapse_os.tools.ticket_tools import _update_ticket_status
    result = asyncio.run(_update_ticket_status(ticket_id, status))
    click.echo(result)


@tool_cli.command("read-kb")
@click.option("--domain", required=True)
@click.option("--key", required=True)
def read_kb(domain, key):
    """Read from knowledge base."""
    from synapse_os.tools.knowledge_tools import _read_knowledge
    result = asyncio.run(_read_knowledge(domain, key))
    click.echo(result)


@tool_cli.command("write-kb")
@click.option("--domain", required=True)
@click.option("--key", required=True)
@click.option("--value", required=True)
@click.option("--expected-version", type=int, default=0)
def write_kb(domain, key, value, expected_version):
    """Write to knowledge base."""
    from synapse_os.tools.knowledge_tools import _write_knowledge
    result = asyncio.run(_write_knowledge(domain, key, value, expected_version))
    click.echo(result)


@tool_cli.command("notify-manager")
@click.option("--ticket-id", required=True)
@click.option("--pr-url", default="")
@click.option("--message", default="")
def notify_manager(ticket_id, pr_url, message):
    """Notify manager of completion or status."""
    from synapse_os.tools.communication_tools import _notify_manager
    result = asyncio.run(_notify_manager(ticket_id, pr_url, message))
    click.echo(result)


@tool_cli.command("status-card")
@click.option("--content", required=True)
@click.option("--priority", type=int, default=0)
def status_card(content, priority):
    """Post a status card."""
    from synapse_os.tools.communication_tools import _post_status_card
    result = asyncio.run(_post_status_card(content, priority))
    click.echo(result)


@tool_cli.command("send-message")
@click.option("--agent-id", required=True)
@click.option("--message", required=True)
def send_message(agent_id, message):
    """Send message to another agent."""
    from synapse_os.tools.communication_tools import _send_agent_message
    result = asyncio.run(_send_agent_message(agent_id, message))
    click.echo(result)


def main():
    tool_cli()


if __name__ == "__main__":
    main()
