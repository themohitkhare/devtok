"""MCP tool server for Synapse OS agents.

Spawned by Claude Code via stdio transport. Connects to Redis
and exposes brain operations as MCP tools.
"""
from __future__ import annotations

import os
import sys
import asyncio
import logging

import redis.asyncio as aioredis
from mcp.server.fastmcp import FastMCP

from synapse_os.brain import Brain

# All logging to stderr — stdout is reserved for JSON-RPC
logging.basicConfig(level=logging.INFO, stream=sys.stderr)
logger = logging.getLogger(__name__)

# Server instance — tools are registered via decorators in other modules
mcp = FastMCP("synapse-os-tools")

# Global state set during startup
_redis: aioredis.Redis | None = None
_brain: Brain | None = None
_agent_id: str = ""
_agent_role: str = ""  # "bootstrap", "manager", "worker"
_manager_id: str = ""
_stdb_client = None


def get_brain() -> Brain:
    assert _brain is not None, "Brain not initialized"
    return _brain


def get_redis() -> aioredis.Redis:
    assert _redis is not None, "Redis not initialized"
    return _redis


def get_agent_id() -> str:
    return _agent_id


def get_agent_role() -> str:
    return _agent_role


def get_manager_id() -> str:
    return _manager_id


async def _init() -> None:
    global _redis, _brain, _agent_id, _agent_role, _manager_id, _stdb_client
    redis_url = os.environ.get("SYNAPSE_REDIS_URL", "redis://localhost:6379/0")
    _redis = aioredis.from_url(redis_url, decode_responses=True)
    _brain = Brain(_redis)
    _agent_id = os.environ.get("SYNAPSE_AGENT_ID", "unknown")
    _agent_role = os.environ.get("SYNAPSE_AGENT_ROLE", "worker")
    _manager_id = os.environ.get("SYNAPSE_MANAGER_ID", "")

    # SpacetimeDB client (optional — for posting ActionCards)
    stdb_url = os.environ.get("SYNAPSE_STDB_URL", "http://localhost:3000")
    stdb_module = os.environ.get("SYNAPSE_STDB_MODULE", "synapse-backend-g9cee")
    try:
        from synapse_os.config import Config
        from synapse_os.spacetimedb_client import SpacetimeDBClient
        cfg = Config(spacetimedb_base_url=stdb_url, spacetimedb_module=stdb_module)
        _stdb_client = SpacetimeDBClient(cfg)
    except Exception:
        logger.warning("SpacetimeDB client not available")

    logger.info("MCP server initialized: agent=%s role=%s", _agent_id, _agent_role)


def _import_tools() -> None:
    """Import tool modules to register their @mcp.tool() decorators."""
    import synapse_os.tools.ticket_tools  # noqa: F401
    import synapse_os.tools.knowledge_tools  # noqa: F401
    import synapse_os.tools.communication_tools  # noqa: F401
    logger.info("All tool modules imported successfully")


# Import tools at module load time so they're registered before mcp.run()
_import_tools()


def main() -> None:
    # Initialize Redis connection synchronously before starting the MCP server
    loop = asyncio.new_event_loop()
    loop.run_until_complete(_init())
    loop.close()

    mcp.run(transport="stdio")


if __name__ == "__main__":
    main()
