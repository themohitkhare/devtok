"""Bootstrap flow: analyze repo/spec → create tickets."""
from __future__ import annotations

import logging
from typing import Optional

from synapse_os.config import Config
from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.spawner import AgentSpawner

logger = logging.getLogger(__name__)


class BootstrapFlow:
    def __init__(self, config: Config, project_dir: str, spec_text: Optional[str] = None) -> None:
        self._config = config
        self._project_dir = project_dir
        self._spec_text = spec_text
        self._spawner = AgentSpawner(config=config, project_dir=project_dir)

    def _build_prompt(self) -> str:
        return build_bootstrap_prompt(repo_path=self._project_dir, spec_text=self._spec_text)

    async def run(self) -> dict:
        logger.info("Starting bootstrap for %s", self._project_dir)
        system_prompt = self._build_prompt()
        task_prompt = (
            f"Analyze the repository at {self._project_dir} and create tickets for all work needed. "
            "Use the create_ticket tool for each piece of work, and write_knowledge_base for key architecture info. "
            "Post a final status_card summarizing what you found."
        )
        proc = await self._spawner.spawn_agent(
            agent_id="bootstrap", role="bootstrap", prompt=task_prompt, system_prompt=system_prompt,
        )
        logger.info("Bootstrap agent spawned (pid=%d), waiting for completion...", proc.pid)
        returncode = await proc.process.wait()
        logger.info("Bootstrap agent finished (exit=%d)", returncode)
        await self._spawner.kill_all()
        return {"status": "complete" if returncode == 0 else "failed", "exit_code": returncode}
