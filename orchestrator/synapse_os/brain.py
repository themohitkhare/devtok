from __future__ import annotations

from redis.asyncio import Redis


class Brain:
    """Redis-backed project brain. Thin wrapper over async Redis with key conventions."""

    def __init__(self, redis: Redis) -> None:
        self._r = redis

    @staticmethod
    def key_agent_inbox(agent_id: str) -> str:
        return f"agent_inbox:{agent_id}"

    @staticmethod
    def key_manager_inbox(manager_id: str) -> str:
        return f"manager_inbox:{manager_id}"

    @staticmethod
    def key_agent_state(agent_id: str) -> str:
        return f"agent_state:{agent_id}"

    @staticmethod
    def key_ticket(ticket_id: str) -> str:
        return f"tickets:{ticket_id}"

    @staticmethod
    def key_standup_responses(manager_id: str) -> str:
        return f"standup_responses:{manager_id}"

    @staticmethod
    def key_knowledge(domain: str, key: str) -> str:
        return f"knowledge_base:{domain}:{key}"

    @staticmethod
    def key_knowledge_ver(key: str) -> str:
        return f"knowledge_base_ver:{key}"

    @staticmethod
    def key_meeting(meeting_id: str) -> str:
        return f"meeting:{meeting_id}"

    @staticmethod
    def key_escalation_card(card_id: str) -> str:
        return f"escalation_card:{card_id}"

    # --- Inbox operations ---

    async def push_inbox(self, agent_id: str, message_json: str) -> None:
        await self._r.rpush(self.key_agent_inbox(agent_id), message_json)

    async def pop_inbox(self, agent_id: str, timeout: int = 0) -> str | None:
        result = await self._r.blpop(self.key_agent_inbox(agent_id), timeout=timeout)
        return result[1] if result else None

    async def push_manager_inbox(self, manager_id: str, message_json: str) -> None:
        await self._r.rpush(self.key_manager_inbox(manager_id), message_json)

    async def pop_manager_inbox(self, manager_id: str, timeout: int = 0) -> str | None:
        result = await self._r.blpop(self.key_manager_inbox(manager_id), timeout=timeout)
        return result[1] if result else None

    # --- Work queue ---

    WORK_QUEUE_KEY = "work_queue"

    _CLAIM_LUA = """
    local ticket_key = KEYS[1]
    local ticket_id = ARGV[1]
    local agent_id = ARGV[2]
    redis.call('HSET', 'in_progress', ticket_id, agent_id)
    redis.call('HSET', ticket_key, 'status', 'in_progress')
    redis.call('HSET', ticket_key, 'assignee', agent_id)
    return 1
    """

    async def enqueue_ticket(self, ticket_id: str) -> None:
        await self._r.rpush(self.WORK_QUEUE_KEY, ticket_id)

    async def claim_ticket(self, timeout: int = 0) -> str | None:
        result = await self._r.blpop(self.WORK_QUEUE_KEY, timeout=timeout)
        return result[1] if result else None

    async def register_claim(self, ticket_id: str, agent_id: str) -> bool:
        result = await self._r.eval(
            self._CLAIM_LUA,
            1,
            self.key_ticket(ticket_id),
            ticket_id,
            agent_id,
        )
        return result == 1

    # --- Knowledge base (optimistic locking) ---

    _KNOWLEDGE_WRITE_LUA = """
    local kb_key = KEYS[1]
    local ver_key = KEYS[2]
    local value = ARGV[1]
    local expected_ver = tonumber(ARGV[2])
    local current_ver = tonumber(redis.call('GET', ver_key) or '0')
    if current_ver ~= expected_ver then
        return 0
    end
    redis.call('SET', kb_key, value)
    redis.call('SET', ver_key, tostring(current_ver + 1))
    return 1
    """

    async def write_knowledge(self, domain: str, key: str, value: str, expected_version: int) -> bool:
        result = await self._r.eval(
            self._KNOWLEDGE_WRITE_LUA,
            2,
            self.key_knowledge(domain, key),
            self.key_knowledge_ver(key),
            value,
            str(expected_version),
        )
        return result == 1

    async def read_knowledge(self, domain: str, key: str) -> tuple[str | None, int]:
        value = await self._r.get(self.key_knowledge(domain, key))
        ver = await self._r.get(self.key_knowledge_ver(key))
        return value, int(ver) if ver else 0
