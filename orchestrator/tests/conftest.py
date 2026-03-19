import pytest
import fakeredis.aioredis


@pytest.fixture
async def redis():
    """Provide a fresh fakeredis async client per test."""
    client = fakeredis.aioredis.FakeRedis(decode_responses=True)
    yield client
    await client.aclose()
