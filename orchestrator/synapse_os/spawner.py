"""Spawns Claude Code CLI instances with MCP tools and system prompts."""
from __future__ import annotations

import json
import os

from synapse_os.config import Config
from synapse_os.process_manager import ManagedProcess, ProcessManager


class AgentSpawner:
    def __init__(self, config: Config, project_dir: str) -> None:
        self._config = config
        self._project_dir = project_dir
        self._pm = ProcessManager()
        self._orchestrator_pkg = os.path.dirname(os.path.abspath(__file__))

    def build_mcp_config(self, agent_id: str, agent_role: str, manager_id: str = "") -> str:
        pkg_root = os.path.dirname(self._orchestrator_pkg)
        config = {
            "mcpServers": {
                "synapse-os-tools": {
                    "type": "stdio",
                    "command": "python3",
                    "args": ["-m", "synapse_os.tools.server"],
                    "env": {
                        "SYNAPSE_REDIS_URL": self._config.redis_url,
                        "SYNAPSE_AGENT_ID": agent_id,
                        "SYNAPSE_AGENT_ROLE": agent_role,
                        "SYNAPSE_MANAGER_ID": manager_id,
                        "SYNAPSE_STDB_URL": self._config.spacetimedb_base_url,
                        "SYNAPSE_STDB_MODULE": self._config.spacetimedb_module,
                        "PYTHONPATH": pkg_root,
                    },
                }
            }
        }
        return json.dumps(config, indent=2)

    def build_claude_command(self, prompt: str, system_prompt: str, allowed_tools: list[str] | None = None) -> list[str]:
        cmd = ["claude", "-p", prompt, "--output-format", "json", "--append-system-prompt", system_prompt]
        if allowed_tools:
            cmd.extend(["--allowedTools", ",".join(allowed_tools)])
        return cmd

    def _write_mcp_config(self, agent_id: str, agent_role: str, manager_id: str) -> str:
        config_content = self.build_mcp_config(agent_id, agent_role, manager_id)
        config_path = os.path.join(self._project_dir, ".mcp.json")
        with open(config_path, "w") as f:
            f.write(config_content)
        return config_path

    async def spawn_agent(self, agent_id: str, role: str, prompt: str, system_prompt: str,
                          manager_id: str = "", allowed_tools: list[str] | None = None) -> ManagedProcess:
        self._write_mcp_config(agent_id, role, manager_id)
        cmd = self.build_claude_command(prompt, system_prompt, allowed_tools)
        return await self._pm.spawn(agent_id=agent_id, command=cmd, cwd=self._project_dir)

    async def kill_agent(self, agent_id: str) -> bool:
        return await self._pm.kill(agent_id)

    async def kill_all(self) -> None:
        await self._pm.kill_all()
