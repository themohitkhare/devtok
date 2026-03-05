#!/usr/bin/env bash
# Synapse — full-stack local launcher
# Starts SpacetimeDB, publishes module, seeds data, starts worker + frontend
set -euo pipefail

export PATH="/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:$HOME/.local/bin:$PATH"
ROOT="$(cd "$(dirname "$0")" && pwd)"
DB_NAME="synapse-backend-g9cee"
DB_URL="http://localhost:3000"

STDB_PID=""
WORKER_PID=""
FRONTEND_PID=""

cleanup() {
  echo ""
  echo "Stopping Synapse..."
  [ -n "$FRONTEND_PID" ] && kill "$FRONTEND_PID" 2>/dev/null || true
  [ -n "$WORKER_PID" ]   && kill "$WORKER_PID"   2>/dev/null || true
  [ -n "$STDB_PID" ]     && kill "$STDB_PID"     2>/dev/null || true
  echo "Done."
}
trap cleanup EXIT INT TERM

# ── 1. SpacetimeDB server ─────────────────────────────────────────────────────
if curl -s --connect-timeout 1 "$DB_URL" >/dev/null 2>&1; then
  echo "✅ SpacetimeDB already running on $DB_URL"
else
  echo "▶ Starting SpacetimeDB..."
  spacetime start &>/tmp/spacetimedb.log &
  STDB_PID=$!
  # Wait up to 15s for server to be ready
  for i in $(seq 1 15); do
    sleep 1
    if curl -s --connect-timeout 1 "$DB_URL" >/dev/null 2>&1; then
      echo "✅ SpacetimeDB ready (${i}s)"
      break
    fi
    if [ "$i" -eq 15 ]; then
      echo "❌ SpacetimeDB failed to start. Check /tmp/spacetimedb.log"
      exit 1
    fi
  done
fi

# ── 2. Publish module ─────────────────────────────────────────────────────────
echo "▶ Publishing SpacetimeDB module..."
cd "$ROOT/backend/synapse-backend"
if echo "N" | spacetime publish "$DB_NAME" --server local 2>&1 | grep -q "Created new database\|already published\|no changes"; then
  echo "✅ Module published"
else
  # Try anyway — might already exist
  echo "N" | spacetime publish "$DB_NAME" --server local 2>&1 || true
  echo "✅ Module publish attempted"
fi
cd "$ROOT"

# ── 3. Seed demo data ─────────────────────────────────────────────────────────
echo "▶ Seeding demo data..."
curl -s -X POST "$DB_URL/v1/database/$DB_NAME/call/seed_demo_data" \
  -H "Content-Type: application/json" -d '[]' >/dev/null
echo "✅ Demo data seeded (skipped if already exists)"

# ── 4. Python worker ─────────────────────────────────────────────────────────
echo "▶ Starting mock agent worker..."
cd "$ROOT/worker"
pip3 install -r requirements.txt --quiet --break-system-packages 2>/dev/null || true
python3 main.py &>/tmp/synapse-worker.log &
WORKER_PID=$!
cd "$ROOT"
echo "✅ Worker started (PID $WORKER_PID) — logs: /tmp/synapse-worker.log"

# ── 5. Frontend dev server ────────────────────────────────────────────────────
echo "▶ Starting frontend..."
cd "$ROOT/frontend"
npm install --silent 2>/dev/null || true
npm run dev &>/tmp/synapse-frontend.log &
FRONTEND_PID=$!
cd "$ROOT"

# Wait for frontend to be ready
for i in $(seq 1 20); do
  sleep 1
  if curl -s --connect-timeout 1 http://localhost:5173 >/dev/null 2>&1; then
    echo "✅ Frontend ready (${i}s)"
    break
  fi
  if [ "$i" -eq 20 ]; then
    echo "⚠  Frontend taking longer than expected — check /tmp/synapse-frontend.log"
  fi
done

# ── Ready ─────────────────────────────────────────────────────────────────────
echo ""
echo "╔═══════════════════════════════════════╗"
echo "║          SYNAPSE is running           ║"
echo "╠═══════════════════════════════════════╣"
echo "║  Frontend  →  http://localhost:5173   ║"
echo "║  Database  →  http://localhost:3000   ║"
echo "║  Module    →  $DB_NAME  ║"
echo "╚═══════════════════════════════════════╝"
echo ""
echo "Logs: /tmp/synapse-worker.log | /tmp/synapse-frontend.log | /tmp/spacetimedb.log"
echo "Press Ctrl+C to stop all services."
echo ""

# Keep alive until interrupted
wait $FRONTEND_PID 2>/dev/null || true
