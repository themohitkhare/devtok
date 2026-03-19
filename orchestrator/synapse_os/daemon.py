from __future__ import annotations

import asyncio
import logging
import signal
import sys

from redis.asyncio import Redis

from synapse_os.brain import Brain
from synapse_os.config import Config
from synapse_os.cron_engine import CronEngine
from synapse_os.feedback_watcher import FeedbackWatcher
from synapse_os.health_checker import HealthChecker
from synapse_os.process_manager import ProcessManager
from synapse_os.registry import AgentRegistry
from synapse_os.spacetimedb_client import SpacetimeDBClient

logger = logging.getLogger(__name__)


class Daemon:
    def __init__(self, config: Config, redis: Redis, stdb_client: SpacetimeDBClient) -> None:
        self._config = config
        self._redis = redis
        self._stdb = stdb_client

        self._brain = Brain(redis)
        self._registry = AgentRegistry(self._brain, redis)
        self._pm = ProcessManager()
        self._health = HealthChecker(
            registry=self._registry, process_manager=self._pm, brain=self._brain,
            heartbeat_timeout=config.heartbeat_timeout_seconds,
            blocked_threshold=config.blocked_threshold_seconds,
        )
        self._feedback = FeedbackWatcher(brain=self._brain, redis=redis, stdb_client=stdb_client)
        self._cron = CronEngine()
        self._feedback_hwm: int = 0
        self._shutting_down = False

        self._cron.register("health_check", config.heartbeat_timeout_seconds, self._health_check_tick)
        self._cron.register("feedback_poll", config.feedback_poll_interval_seconds, self._feedback_poll_tick)

    async def run(self) -> None:
        logger.info("Synapse OS daemon starting")
        try:
            await self._cron.run()
        finally:
            await self._cleanup()

    def shutdown(self) -> None:
        logger.info("Shutdown requested")
        self._shutting_down = True
        self._cron.stop()

    async def _cleanup(self) -> None:
        logger.info("Cleaning up: killing all agent processes")
        await self._pm.kill_all()
        await self._stdb.close()
        logger.info("Daemon stopped")

    async def _health_check_tick(self) -> None:
        crashed = self._health.check_crashed()
        for proc in crashed:
            logger.warning("Agent %s crashed (pid %d), removing from registry", proc.agent_id, proc.pid)
            await self._registry.deregister(proc.agent_id)
            self._pm.processes.pop(proc.agent_id, None)

        stale = await self._health.check_heartbeats()
        for agent in stale:
            logger.warning("Agent %s heartbeat stale", agent.agent_id)

        blocked = await self._health.check_blocked()
        for agent in blocked:
            if agent.manager_id:
                from synapse_os.models import InboxMessage
                msg = InboxMessage(
                    msg_type="agent_blocked",
                    payload={"agent_id": agent.agent_id, "status": agent.status},
                    sender="orchestrator",
                )
                await self._brain.push_manager_inbox(agent.manager_id, msg.to_json())

    async def _feedback_poll_tick(self) -> None:
        self._feedback_hwm = await self._feedback.poll_once(self._feedback_hwm)


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(name)s] %(levelname)s: %(message)s")

    config = Config.from_env()

    async def _run() -> None:
        redis = Redis.from_url(config.redis_url, decode_responses=True)
        stdb = SpacetimeDBClient(config)
        daemon = Daemon(config=config, redis=redis, stdb_client=stdb)

        loop = asyncio.get_event_loop()
        loop.add_signal_handler(signal.SIGINT, daemon.shutdown)
        loop.add_signal_handler(signal.SIGTERM, daemon.shutdown)

        await daemon.run()
        await redis.aclose()

    asyncio.run(_run())


if __name__ == "__main__":
    main()
