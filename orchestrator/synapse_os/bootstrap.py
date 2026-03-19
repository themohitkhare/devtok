"""Bootstrap flow: analyze repo/spec → create tickets."""
from __future__ import annotations

import logging
import os
from typing import Optional

from synapse_os.config import Config
from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.spawner import AgentSpawner, _find_venv_python

logger = logging.getLogger(__name__)


class BootstrapFlow:
    def __init__(self, config: Config, project_dir: str, spec_text: Optional[str] = None) -> None:
        self._config = config
        self._project_dir = project_dir
        self._spec_text = spec_text
        self._spawner = AgentSpawner(config=config, project_dir=project_dir)

        # Build the synapse-tool path (in the venv)
        venv_python = _find_venv_python()
        venv_bin = os.path.dirname(venv_python)
        self._tool_path = os.path.join(venv_bin, "synapse-tool")

    def _build_prompt(self) -> str:
        return build_bootstrap_prompt(
            repo_path=self._project_dir,
            spec_text=self._spec_text,
            tool_path=self._tool_path,
        )

    async def run(self) -> dict:
        logger.info("Starting bootstrap for %s", self._project_dir)
        system_prompt = self._build_prompt()
        task_prompt = (
            f"Analyze the repository at {self._project_dir} and create tickets for all work needed. "
            f"Use the Bash tool to run `{self._tool_path} create-ticket` for each piece of work. "
            f"Use `{self._tool_path} write-kb` for key architecture info. "
            f"Use `{self._tool_path} status-card` to post a final summary. "
            "IMPORTANT: You MUST use the Bash tool to call synapse-tool commands. Do NOT try MCP tools."
        )

        # Set env vars so synapse-tool can connect to Redis
        os.environ["SYNAPSE_REDIS_URL"] = self._config.redis_url
        os.environ["SYNAPSE_AGENT_ID"] = "bootstrap"
        os.environ["SYNAPSE_AGENT_ROLE"] = "bootstrap"

        proc = await self._spawner.spawn_agent(
            agent_id="bootstrap", role="bootstrap", prompt=task_prompt, system_prompt=system_prompt,
        )
        logger.info("Bootstrap agent spawned (pid=%d), waiting for completion...", proc.pid)
        returncode = await proc.process.wait()
        logger.info("Bootstrap agent finished (exit=%d)", returncode)
        await self._spawner.kill_all()
        return {"status": "complete" if returncode == 0 else "failed", "exit_code": returncode}
