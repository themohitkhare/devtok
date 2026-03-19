"""CLI entry points for Synapse OS."""
from __future__ import annotations

import asyncio
import logging
import os

import click

from synapse_os.config import Config


@click.group()
def cli():
    """Synapse OS — Autonomous Project Management"""
    logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(name)s] %(levelname)s: %(message)s")


@cli.command()
@click.argument("path", type=click.Path(exists=True))
@click.option("--spec", type=click.Path(exists=True), help="Path to spec/brief file")
def init(path: str, spec: str | None):
    """Bootstrap a project: analyze repo and create initial tickets."""
    from synapse_os.bootstrap import BootstrapFlow

    config = Config.from_env()
    abs_path = os.path.abspath(path)
    spec_text = None
    if spec:
        with open(spec) as f:
            spec_text = f.read()

    click.echo(f"Bootstrapping project at {abs_path}...")
    flow = BootstrapFlow(config=config, project_dir=abs_path, spec_text=spec_text)
    result = asyncio.run(flow.run())

    if result["status"] == "complete":
        click.echo("Bootstrap complete! Tickets created.")
    else:
        click.echo(f"Bootstrap failed (exit code {result['exit_code']})")
        raise SystemExit(1)


@cli.command()
@click.argument("path", type=click.Path(exists=True))
@click.option("--domain", default="general", help="Domain for this team")
@click.option("--workers", default=2, type=int, help="Number of worker agents")
def run(path: str, domain: str, workers: int):
    """Start manager + workers to execute tickets."""
    import redis.asyncio as aioredis
    from synapse_os.brain import Brain
    from synapse_os.runner import Runner

    config = Config.from_env()
    abs_path = os.path.abspath(path)

    async def _run():
        r = aioredis.from_url(config.redis_url, decode_responses=True)
        brain = Brain(r)
        runner = Runner(config=config, project_dir=abs_path, redis=r, brain=brain, domain=domain, num_workers=workers)
        click.echo(f"Starting {domain} team with {workers} workers...")
        await runner.start()
        click.echo("Agents running. Press Ctrl+C to stop.")
        try:
            await runner.run_loop()
        except KeyboardInterrupt:
            pass
        finally:
            await runner.stop()
            await r.aclose()
            click.echo("Stopped.")

    asyncio.run(_run())


@cli.command()
def preflight():
    """Check that all dependencies are available."""
    import shutil
    import subprocess

    ok = True

    # Check Redis
    try:
        result = subprocess.run(["redis-cli", "ping"], capture_output=True, text=True, timeout=5)
        if result.stdout.strip() == "PONG":
            click.echo("  [OK] Redis is running")
        else:
            click.echo("  [FAIL] Redis not responding. Run: redis-server --daemonize yes")
            ok = False
    except (FileNotFoundError, subprocess.TimeoutExpired):
        click.echo("  [FAIL] redis-cli not found. Install Redis first.")
        ok = False

    # Check Claude CLI
    if shutil.which("claude"):
        click.echo("  [OK] Claude Code CLI found")
    else:
        click.echo("  [FAIL] 'claude' not in PATH. Install Claude Code CLI.")
        ok = False

    # Check Python venv
    from synapse_os.spawner import _find_venv_python
    venv_py = _find_venv_python()
    try:
        result = subprocess.run([venv_py, "-c", "import mcp; print('ok')"], capture_output=True, text=True, timeout=10)
        if "ok" in result.stdout:
            click.echo(f"  [OK] MCP package available ({venv_py})")
        else:
            click.echo(f"  [FAIL] MCP package not importable in {venv_py}")
            ok = False
    except Exception:
        click.echo(f"  [FAIL] Cannot run {venv_py}")
        ok = False

    if ok:
        click.echo("\nAll checks passed! Ready to go.")
    else:
        click.echo("\nSome checks failed. Fix the issues above first.")
        raise SystemExit(1)


@cli.command()
def status():
    """Show current project status."""
    import redis.asyncio as aioredis
    from synapse_os.brain import Brain
    from synapse_os.registry import AgentRegistry

    config = Config.from_env()

    async def _status():
        r = aioredis.from_url(config.redis_url, decode_responses=True)
        brain = Brain(r)
        registry = AgentRegistry(brain, r)
        agents = await registry.list_agents()
        queue_len = await r.llen(Brain.WORK_QUEUE_KEY)
        click.echo(f"Registered agents: {len(agents)}")
        for a in agents:
            click.echo(f"  {a.agent_id} ({a.role.value}) — {a.status}")
        click.echo(f"Work queue: {queue_len} tickets pending")
        await r.aclose()

    asyncio.run(_status())


def main():
    cli()
