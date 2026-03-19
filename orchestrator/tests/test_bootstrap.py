import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.bootstrap import BootstrapFlow
from synapse_os.config import Config

pytestmark = pytest.mark.asyncio


async def test_bootstrap_builds_correct_prompt():
    flow = BootstrapFlow(config=Config(), project_dir="/tmp/myrepo", spec_text="Build a todo app")
    prompt = flow._build_prompt()
    assert "/tmp/myrepo" in prompt
    assert "todo app" in prompt


async def test_bootstrap_builds_prompt_without_spec():
    flow = BootstrapFlow(config=Config(), project_dir="/tmp/myrepo")
    prompt = flow._build_prompt()
    assert "/tmp/myrepo" in prompt


async def test_bootstrap_run_spawns_agent():
    flow = BootstrapFlow(config=Config(), project_dir="/tmp/myrepo", spec_text="Build a todo app")
    mock_proc = MagicMock()
    mock_proc.agent_id = "bootstrap"
    mock_proc.pid = 999
    mock_proc.is_running = False
    mock_proc.process = MagicMock()
    mock_proc.process.wait = AsyncMock(return_value=0)
    mock_proc.process.returncode = 0

    with patch.object(flow._spawner, 'spawn_agent', new_callable=AsyncMock, return_value=mock_proc), \
         patch.object(flow._spawner, 'kill_all', new_callable=AsyncMock):
        result = await flow.run()
        assert result["status"] == "complete"
