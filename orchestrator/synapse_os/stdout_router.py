from __future__ import annotations

from typing import Any, Protocol

from synapse_os.spacetimedb_client import SpacetimeDBClient


class ReadableStream(Protocol):
    async def readline(self) -> bytes: ...


class StdoutRouter:
    def __init__(self, stdb_client: SpacetimeDBClient, project_id: int, stdb_agent_id: int, batch_interval: float = 0.5) -> None:
        self._stdb = stdb_client
        self._project_id = project_id
        self._stdb_agent_id = stdb_agent_id
        self._batch_interval = batch_interval

    async def route_stream(self, stream: ReadableStream, agent_id: str, batch: bool = False) -> None:
        if batch:
            await self._route_batched(stream, agent_id)
        else:
            await self._route_line_by_line(stream, agent_id)

    async def _route_line_by_line(self, stream: ReadableStream, agent_id: str) -> None:
        while True:
            line = await stream.readline()
            if not line:
                break
            text = line.decode("utf-8", errors="replace").rstrip("\n")
            if text:
                await self._post_card(agent_id, text)

    async def _route_batched(self, stream: ReadableStream, agent_id: str) -> None:
        lines: list[str] = []
        while True:
            line = await stream.readline()
            if not line:
                break
            text = line.decode("utf-8", errors="replace").rstrip("\n")
            if text:
                lines.append(text)
        if lines:
            await self._post_card(agent_id, "\n".join(lines))

    async def _post_card(self, agent_id: str, content: str) -> None:
        await self._stdb.post_action_card(
            agent_id=self._stdb_agent_id, project_id=self._project_id,
            visual_type="TerminalOutput", content=content,
            task_summary=f"Output from {agent_id}", priority=0,
        )
