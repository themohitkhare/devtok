from __future__ import annotations

import asyncio
from dataclasses import dataclass


@dataclass
class ManagedProcess:
    agent_id: str
    command: list[str]
    process: asyncio.subprocess.Process
    cwd: str

    @property
    def pid(self) -> int:
        return self.process.pid

    @property
    def is_running(self) -> bool:
        return self.process.returncode is None


class ProcessManager:
    def __init__(self) -> None:
        self.processes: dict[str, ManagedProcess] = {}

    async def spawn(self, agent_id: str, command: list[str], cwd: str) -> ManagedProcess:
        proc = await asyncio.create_subprocess_exec(
            *command, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE, cwd=cwd,
        )
        managed = ManagedProcess(agent_id=agent_id, command=command, process=proc, cwd=cwd)
        self.processes[agent_id] = managed
        return managed

    async def kill(self, agent_id: str) -> bool:
        managed = self.processes.pop(agent_id, None)
        if not managed:
            return False
        if managed.is_running:
            managed.process.terminate()
            try:
                await asyncio.wait_for(managed.process.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                managed.process.kill()
                await managed.process.wait()
        return True

    async def kill_all(self) -> None:
        agent_ids = list(self.processes.keys())
        for aid in agent_ids:
            await self.kill(aid)

    def list_running(self) -> list[ManagedProcess]:
        return [p for p in self.processes.values() if p.is_running]

    def get_crashed(self) -> list[ManagedProcess]:
        return [p for p in self.processes.values() if not p.is_running]
