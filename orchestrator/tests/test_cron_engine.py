import asyncio
import pytest
from synapse_os.cron_engine import CronEngine

pytestmark = pytest.mark.asyncio


async def test_registers_and_fires_job():
    fired = []

    async def my_job():
        fired.append(True)

    engine = CronEngine()
    engine.register("test_job", interval_seconds=0.1, callback=my_job)
    task = asyncio.create_task(engine.run())

    await asyncio.sleep(0.35)
    engine.stop()
    await task

    assert len(fired) >= 2


async def test_multiple_jobs_different_intervals():
    fast_count = []
    slow_count = []

    async def fast_job():
        fast_count.append(1)

    async def slow_job():
        slow_count.append(1)

    engine = CronEngine()
    engine.register("fast", interval_seconds=0.05, callback=fast_job)
    engine.register("slow", interval_seconds=0.2, callback=slow_job)
    task = asyncio.create_task(engine.run())

    await asyncio.sleep(0.5)
    engine.stop()
    await task

    assert len(fast_count) > len(slow_count)


async def test_stop_is_clean():
    async def noop():
        pass

    engine = CronEngine()
    engine.register("noop", interval_seconds=0.1, callback=noop)
    task = asyncio.create_task(engine.run())
    engine.stop()
    await task  # Should not hang
