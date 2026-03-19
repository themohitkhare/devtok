import asyncio
import pytest
from synapse_os.process_manager import ProcessManager, ManagedProcess

pytestmark = pytest.mark.asyncio


async def test_spawn_process():
    pm = ProcessManager()
    proc = await pm.spawn(agent_id="test-1", command=["python3", "-c", "import time; time.sleep(30)"], cwd="/tmp")
    assert proc.agent_id == "test-1"
    assert proc.process.returncode is None
    await pm.kill("test-1")


async def test_kill_process():
    pm = ProcessManager()
    await pm.spawn("test-1", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    success = await pm.kill("test-1")
    assert success is True
    assert "test-1" not in pm.processes


async def test_kill_nonexistent():
    pm = ProcessManager()
    success = await pm.kill("no-such-agent")
    assert success is False


async def test_list_processes():
    pm = ProcessManager()
    await pm.spawn("a1", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    await pm.spawn("a2", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    procs = pm.list_running()
    assert len(procs) == 2
    await pm.kill_all()


async def test_detect_crashed():
    pm = ProcessManager()
    await pm.spawn("crash-1", ["python3", "-c", "exit(1)"], "/tmp")
    await asyncio.sleep(0.2)
    crashed = pm.get_crashed()
    assert "crash-1" in [p.agent_id for p in crashed]
    await pm.kill_all()


async def test_get_pid():
    pm = ProcessManager()
    proc = await pm.spawn("test-1", ["python3", "-c", "import time; time.sleep(30)"], "/tmp")
    assert proc.pid > 0
    await pm.kill_all()
