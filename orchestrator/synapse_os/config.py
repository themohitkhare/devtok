import os
from dataclasses import dataclass


@dataclass
class Config:
    redis_url: str = "redis://localhost:6379/0"
    spacetimedb_base_url: str = "http://localhost:3000"
    spacetimedb_module: str = "synapse-backend-g9cee"
    standup_interval_seconds: int = 86400
    heartbeat_timeout_seconds: int = 60
    blocked_threshold_seconds: int = 1800
    feedback_poll_interval_seconds: int = 5
    escalation_timeout_hours: int = 24

    @classmethod
    def from_env(cls) -> "Config":
        return cls(
            redis_url=os.getenv("SYNAPSE_REDIS_URL", cls.redis_url),
            spacetimedb_base_url=os.getenv("SYNAPSE_STDB_URL", cls.spacetimedb_base_url),
            spacetimedb_module=os.getenv("SYNAPSE_STDB_MODULE", cls.spacetimedb_module),
            standup_interval_seconds=int(os.getenv("SYNAPSE_STANDUP_INTERVAL", cls.standup_interval_seconds)),
            heartbeat_timeout_seconds=int(os.getenv("SYNAPSE_HEARTBEAT_TIMEOUT", cls.heartbeat_timeout_seconds)),
            blocked_threshold_seconds=int(os.getenv("SYNAPSE_BLOCKED_THRESHOLD", cls.blocked_threshold_seconds)),
            feedback_poll_interval_seconds=int(os.getenv("SYNAPSE_FEEDBACK_POLL", cls.feedback_poll_interval_seconds)),
            escalation_timeout_hours=int(os.getenv("SYNAPSE_ESCALATION_TIMEOUT", cls.escalation_timeout_hours)),
        )
