from __future__ import annotations

from typing import Any

import httpx

from synapse_os.config import Config


class SpacetimeDBClient:
    def __init__(self, config: Config, transport: httpx.AsyncBaseTransport | None = None) -> None:
        self._base = f"{config.spacetimedb_base_url}/v1/database/{config.spacetimedb_module}"
        kwargs: dict[str, Any] = {"timeout": 10.0}
        if transport:
            kwargs["transport"] = transport
        self._http = httpx.AsyncClient(**kwargs)

    async def call_reducer(self, reducer: str, args: list[Any]) -> None:
        url = f"{self._base}/call/{reducer}"
        resp = await self._http.post(url, json=args, headers={"Content-Type": "application/json"})
        resp.raise_for_status()

    async def query_sql(self, sql: str) -> list[list[Any]]:
        url = f"{self._base}/sql"
        resp = await self._http.post(url, content=sql, headers={"Content-Type": "text/plain"})
        resp.raise_for_status()
        chunks = resp.json()
        rows: list[list[Any]] = []
        for chunk in chunks:
            rows.extend(chunk.get("rows", []))
        return rows

    async def post_action_card(self, agent_id: int, project_id: int, visual_type: str,
                                content: str, task_summary: str, priority: int) -> None:
        await self.call_reducer("insert_action_card", [agent_id, project_id, visual_type, content, task_summary, priority])

    async def query_feedback_since(self, since_micros: int) -> list[list[Any]]:
        sql = f"SELECT id, card_id, action_type, payload, created_at FROM feedback WHERE created_at > {since_micros}"
        return await self.query_sql(sql)

    async def close(self) -> None:
        await self._http.aclose()
