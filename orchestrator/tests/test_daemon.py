import asyncio
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from synapse_os.config import Config
from synapse_os.daemon import Daemon

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
