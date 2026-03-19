import asyncio
import json
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.config import Config
from synapse_os.daemon import Daemon
from synapse_os.models import AgentRole

pytestmark = pytest.mark.asyncio


async def test_daemon_starts_and_stops(redis):
    cfg = Config()
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    task = asyncio.create_task(daemon.run())
    await asyncio.sleep(0.3)
    daemon.shutdown()
    await asyncio.wait_for(task, timeout=5.0)


async def test_daemon_registers_cron_jobs(redis):
    cfg = Config()
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    job_names = [j.name for j in daemon._cron._jobs]
    assert "health_check" in job_names
    assert "feedback_poll" in job_names
    assert "standup" in job_names


async def test_standup_tick_sends_requests_to_workers(redis):
    cfg = Config(standup_interval_seconds=86400)
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    # Register two workers under the same manager
    await daemon._registry.register("w-1", AgentRole.WORKER, "general", "mgr-1", 1001)
    await daemon._registry.register("w-2", AgentRole.WORKER, "general", "mgr-1", 1002)

    await daemon._standup_tick()

    # Each worker should have a standup_request in their inbox
    raw1 = await redis.lpop("agent_inbox:w-1")
    raw2 = await redis.lpop("agent_inbox:w-2")
    assert raw1 is not None
    assert raw2 is not None

    msg1 = json.loads(raw1)
    msg2 = json.loads(raw2)
    assert msg1["msg_type"] == "standup_request"
    assert msg1["payload"]["manager_id"] == "mgr-1"
    assert "deadline_epoch" in msg1["payload"]
    assert msg2["msg_type"] == "standup_request"

    # Manager should receive exactly one standup_tick (not two)
    mgr_raw = await redis.lpop("manager_inbox:mgr-1")
    assert mgr_raw is not None
    mgr_msg = json.loads(mgr_raw)
    assert mgr_msg["msg_type"] == "standup_tick"

    # No duplicate standup_tick
    assert await redis.lpop("manager_inbox:mgr-1") is None


async def test_standup_tick_multiple_managers(redis):
    cfg = Config(standup_interval_seconds=86400)
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    await daemon._registry.register("w-1", AgentRole.WORKER, "frontend", "mgr-fe", 2001)
    await daemon._registry.register("w-2", AgentRole.WORKER, "backend", "mgr-be", 2002)

    await daemon._standup_tick()

    # Both managers should get a standup_tick
    mgr_fe = await redis.lpop("manager_inbox:mgr-fe")
    mgr_be = await redis.lpop("manager_inbox:mgr-be")
    assert mgr_fe is not None
    assert mgr_be is not None
    assert json.loads(mgr_fe)["msg_type"] == "standup_tick"
    assert json.loads(mgr_be)["msg_type"] == "standup_tick"


async def test_standup_tick_no_workers(redis):
    cfg = Config(standup_interval_seconds=86400)
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    # No workers registered — should complete without error
    await daemon._standup_tick()


async def test_standup_interval_uses_config(redis):
    cfg = Config(standup_interval_seconds=3600)
    stdb = AsyncMock()
    stdb.query_feedback_since = AsyncMock(return_value=[])
    stdb.close = AsyncMock()

    daemon = Daemon(config=cfg, redis=redis, stdb_client=stdb)

    standup_job = next(j for j in daemon._cron._jobs if j.name == "standup")
    assert standup_job.interval_seconds == 3600
