"""MCP tools for the shared knowledge base."""
from __future__ import annotations

import json

from synapse_os.tools.server import get_brain, mcp


@mcp.tool()
async def read_knowledge_base(domain: str, key: str) -> str:
    """Read a value from the shared project knowledge base.

    Args:
        domain: Domain prefix (e.g. 'frontend', 'backend', 'contract')
        key: The key to read
    """
    return await _read_knowledge(domain, key)


async def _read_knowledge(domain: str, key: str) -> str:
    brain = get_brain()
    value, version = await brain.read_knowledge(domain, key)
    return json.dumps({"domain": domain, "key": key, "value": value, "version": version})


@mcp.tool()
async def write_knowledge_base(domain: str, key: str, value: str, expected_version: int = 0) -> str:
    """Write a value to the shared project knowledge base with optimistic locking.

    Args:
        domain: Domain prefix (e.g. 'frontend', 'backend', 'contract')
        key: The key to write
        value: The value to store (typically JSON string)
        expected_version: Expected current version (0 for new keys). Write fails if version doesn't match.
    """
    return await _write_knowledge(domain, key, value, expected_version)


async def _write_knowledge(domain: str, key: str, value: str, expected_version: int = 0) -> str:
    brain = get_brain()
    ok = await brain.write_knowledge(domain, key, value, expected_version)
    if ok:
        _, new_ver = await brain.read_knowledge(domain, key)
        return json.dumps({"status": "written", "new_version": new_ver})
    return json.dumps({"status": "conflict", "message": "Version mismatch. Re-read and retry."})
