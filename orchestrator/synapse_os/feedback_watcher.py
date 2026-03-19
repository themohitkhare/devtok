from __future__ import annotations

import json
import logging

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.models import InboxMessage
from synapse_os.spacetimedb_client import SpacetimeDBClient

logger = logging.getLogger(__name__)


class FeedbackWatcher:
    def __init__(self, brain: Brain, redis: Redis, stdb_client: SpacetimeDBClient) -> None:
        self._brain = brain
        self._r = redis
        self._stdb = stdb_client

    async def poll_once(self, since_micros: int) -> int:
        rows = await self._stdb.query_feedback_since(since_micros)
        high_water = since_micros

        for row in rows:
            feedback_id, card_id, action_type, payload, created_at = row

            if created_at > high_water:
                high_water = created_at

            manager_id = await self._r.get(self._brain.key_escalation_card(str(card_id)))
            if not manager_id:
                continue

            approved = action_type == "approve"
            msg = InboxMessage(
                msg_type="escalation_response",
                payload={"approved": approved, "comment": payload or "", "card_id": card_id},
                sender="human",
            )
            await self._brain.push_manager_inbox(manager_id, msg.to_json())
            logger.info("Forwarded escalation response for card %s to manager %s", card_id, manager_id)

        return high_water
