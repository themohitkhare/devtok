from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from typing import Awaitable, Callable

logger = logging.getLogger(__name__)


@dataclass
class CronJob:
    name: str
    interval_seconds: float
    callback: Callable[[], Awaitable[None]]


class CronEngine:
    def __init__(self) -> None:
        self._jobs: list[CronJob] = []
        self._stop_event = asyncio.Event()

    def register(self, name: str, interval_seconds: float, callback: Callable[[], Awaitable[None]]) -> None:
        self._jobs.append(CronJob(name=name, interval_seconds=interval_seconds, callback=callback))

    def stop(self) -> None:
        self._stop_event.set()

    async def run(self) -> None:
        if self._stop_event.is_set():
            return
        tasks = [asyncio.create_task(self._run_job(job)) for job in self._jobs]
        await self._stop_event.wait()
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)

    async def _run_job(self, job: CronJob) -> None:
        while not self._stop_event.is_set():
            try:
                await job.callback()
            except Exception:
                logger.exception("Cron job %s failed", job.name)
            await asyncio.sleep(job.interval_seconds)
