import json
import os
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.spawner import AgentSpawner
from synapse_os.config import Config

pytestmark = pytest.mark.asyncio


def test_build_mcp_config():
    spawner = AgentSpawner(config=Config(), project_dir="/tmp/myproject")
    config = spawner.build_mcp_config(agent_id="w-1", agent_role="worker", manager_id="m-1")
    parsed = json.loads(config)
    server = parsed["mcpServers"]["synapse-os-tools"]
    assert server["type"] == "stdio"
    assert "synapse_os.tools.server" in " ".join(server["args"])
    assert server["env"]["SYNAPSE_AGENT_ID"] == "w-1"
    assert server["env"]["SYNAPSE_AGENT_ROLE"] == "worker"
    assert server["env"]["SYNAPSE_MANAGER_ID"] == "m-1"


def test_build_claude_command():
    spawner = AgentSpawner(config=Config(), project_dir="/tmp/myproject")
    cmd = spawner.build_claude_command(prompt="Do the work", system_prompt="You are a worker", allowed_tools=None)
    assert "claude" in cmd[0]
    assert "-p" in cmd
    assert "--output-format" in cmd
    assert "json" in cmd
    assert "--append-system-prompt" in cmd


async def test_spawn_agent():
    spawner = AgentSpawner(config=Config(), project_dir="/tmp/myproject")
    mock_proc = MagicMock()
    mock_proc.agent_id = "w-1"
    mock_proc.pid = 12345
    mock_proc.process = MagicMock()
    mock_proc.process.stdout = MagicMock()
    spawner._pm = AsyncMock()
    spawner._pm.spawn = AsyncMock(return_value=mock_proc)

    with patch.object(spawner, '_write_mcp_config'):
        result = await spawner.spawn_agent(
            agent_id="w-1", role="worker", prompt="Build the login page",
            system_prompt="You are a worker", manager_id="m-1",
        )
        assert result.agent_id == "w-1"
        spawner._pm.spawn.assert_called_once()
