# 005 - Run Autoscaling Workers

## Background

`acs run` currently starts a fixed worker count and keeps all workers alive for the full run.
This causes over-provisioning when ticket load is low and slower ramp-up when load spikes.

## Problem

Add runtime autoscaling so worker count can grow and shrink safely while ACS is running.

## Questions and Answers

### Q1: Should autoscaling be default-on?
**A:** No. Keep existing behavior unchanged by default to avoid surprising users. Autoscaling is opt-in via a flag.

### Q2: What should `--workers` mean with autoscaling?
**A:** `--workers` is treated as the autoscaling maximum. A new `--min-workers` controls the floor.

### Q3: What queue signal drives scaling decisions?
**A:** Queue depth = `pending + in_progress + review_pending`.

### Q4: How should downscaling avoid interrupting active work?
**A:** Downscale only idle workers. Busy workers are never force-stopped by autoscaler.

## Design

- Add `acs run --autoscale --min-workers <n> --workers <max>`.
- Scaling loop runs every 2 seconds in `src/cli/run.rs`.
- Desired workers:
  - `desired = clamp(queue_depth, min_workers, max_workers)`
- Scale-up:
  - Register worker agent in DB
  - Spawn worker loop task
- Scale-down:
  - Query DB agents and select only `status == "idle"`
  - Stop highest-index idle workers first
  - Deregister removed workers from DB
- Shutdown:
  - Stop manager
  - Stop all active workers
  - Cleanup worktrees/branches

## Implementation Plan

1. Add CLI flags to `Run` command: `autoscale`, `min_workers`.
2. Update command dispatch in `src/main.rs`.
3. Refactor `src/cli/run.rs`:
   - Track per-worker task handles and shutdown channels
   - Add periodic autoscale reconciliation
   - Add helper for desired worker computation
4. Add unit tests for scaling helper behavior.

## Examples

✅ Scale between 2 and 10 workers:

`acs run --autoscale --min-workers 2 --workers 10 --backend cursor`

✅ Keep old fixed behavior:

`acs run --workers 5 --backend cursor`

❌ Invalid expectation:

`acs run --autoscale --min-workers 8 --workers 4`

In this case min is clamped to max at runtime.

## Trade-offs

- Pros:
  - Better utilization for long-running ACS sessions
  - Faster response to queue growth without manual restarts
- Cons:
  - More runtime state to track in `run.rs`
  - Downscale waits for idleness, so actual active count may temporarily exceed desired
