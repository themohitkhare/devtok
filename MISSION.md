# MISSION: Complete Synapse to 100%

> **If you are an AI agent reading this file: you must continue working until every item in the REMAINING WORK checklist below is marked ✅. Do not stop. Do not summarize and wait. Execute each item, verify it, then move to the next.**

---

## Parallel Agent Orchestration Protocol

### How to spawn parallel cursor agents
```bash
# Spawn an agent in background (tracked terminal file)
cursor agent --print --yolo --approve-mcps --workspace /Users/mkhare/Development/devtok/<dir> \
  "<prompt>" &

# Spawn multiple agents in parallel at once (NO & within the shell block — use block_until_ms:0)
# Each gets its own workspace dir so they don't conflict on files
```

### Mandatory flags for all spawned agents
```
--print          headless/scripted output
--yolo           execute all tools without prompting
--approve-mcps   auto-approve all MCP servers (memory, paper, etc.)
--workspace      set to the specific subdirectory the agent owns
```

### Memory MCP — read project context
All agents MUST read these entities first via user-memory MCP:
```
Synapse_Project       — concept, goals, directory layout
Synapse_TechStack     — tech stack decisions
Synapse_Schema        — SpacetimeDB tables + reducers  
Synapse_VisualDesign  — Cyber-Glass Dark Mode design tokens
Synapse_Components    — component tree and interfaces
Synapse_AgentStates   — orbital task lights implementation
```

MCP tool calls to read them:
```json
{ "server": "user-memory", "toolName": "open_nodes", "arguments": { "names": ["Synapse_Project","Synapse_TechStack","Synapse_Schema","Synapse_VisualDesign","Synapse_Components","Synapse_AgentStates"] } }
```

### Memory MCP — write progress back
After completing work, write results to memory so other agents know:
```json
{ "server": "user-memory", "toolName": "add_observations", "arguments": { "observations": [{ "entityName": "Synapse_Project", "contents": ["<what you completed>"] }] } }
```

### Paper MCP — visual design (requires Paper.design app open)
Tools available: `get_basic_info`, `create_artboard`, `write_html`, `get_screenshot`, `finish_working_on_nodes`
Server: `user-paper`
Use for: creating the mobile (390x844) and desktop (1440x900) artboards

### Superpowers skills to invoke
Read these skill files before relevant tasks:
- `/Users/mkhare/.cursor/plugins/cache/cursor-public/superpowers/a0b9ecce2b25aa7d703138f17650540c2e8b2cde/skills/subagent-driven-development/SKILL.md`
- `/Users/mkhare/.cursor/plugins/cache/cursor-public/superpowers/a0b9ecce2b25aa7d703138f17650540c2e8b2cde/skills/dispatching-parallel-agents/SKILL.md`
- `/Users/mkhare/.cursor/plugins/cache/cursor-public/superpowers/a0b9ecce2b25aa7d703138f17650540c2e8b2cde/skills/verification-before-completion/SKILL.md`

### Parallel task split strategy
Whenever 2+ independent tasks exist, split them:
- **Agent A** owns `frontend/` — TypeScript/React work
- **Agent B** owns `backend/` — Rust/SpacetimeDB work  
- **Agent C** owns `worker/` — Python work
- **Main agent** owns orchestration, `design-log/`, `docs/`, `MISSION.md`
Never assign the same directory to two agents simultaneously.

---

## Project: Synapse
TikTok-style vertical snap-scroll feed for monitoring local AI agent tasks.
**Workspace:** `/Users/mkhare/Development/devtok`

### Directory layout
```
devtok/
├── backend/synapse-backend/spacetimedb/src/lib.rs   ← Rust module
├── frontend/src/                                     ← React/Vite/TS
│   ├── components/ui/   ← AgentProfileRing, CodeDiffViewer, etc.
│   ├── hooks/useSpacetimeDB.ts
│   ├── mock-data.ts
│   └── types.ts
├── worker/main.py                                    ← Python HTTP worker
├── design-log/001-synapse-architecture.md
├── docs/plans/                                       ← required by project rules
├── start.sh                                          ← full-stack launcher
└── MISSION.md                                        ← this file
```

---

## Current State (2026-03-05)

### ✅ Done
- SpacetimeDB CLI v2.0.3 installed at `~/.local/bin/spacetime`
- Rust module compiled to WASM — 5 tables, 10 reducers — **0 build errors**
- Module published as `synapse-backend-g9cee` on `localhost:3000`
- `seed_demo_data` verified: 3 agents + 5 cards in DB
- React frontend builds (`npm run build` → 0 errors, 341KB bundle)
- All components written: AgentProfileRing (SVG orbital dots), Feed (Framer Motion drag), ActionCard, CodeDiffViewer, TerminalGlassPanel, InteractionSidebar, BottomOverlay, StatusBadge
- Python worker fixed: correct module name `synapse-backend-g9cee`, correct reducer arg order, verified inserting live cards to DB

---

## REMAINING WORK

> Work these in order. Mark ✅ when each is verified working. Use parallel agents where indicated.

### ✅ Item 3 — Install SpacetimeDB TypeScript SDK + live subscription
**Agent: A (frontend)**
```bash
cd /Users/mkhare/Development/devtok/frontend
npm install @clockworklabs/spacetimedb-sdk
```

Rewrite `src/hooks/useSpacetimeDB.ts` to use the actual SDK. Pattern:
```typescript
import { DbConnection, DbConnectionBuilder } from '@clockworklabs/spacetimedb-sdk'
// Connect to ws://localhost:3000, module identity 'synapse-backend-g9cee'
// Subscribe to ActionCard table
// On row insert/update: update cards state (sorted by priority desc)
// On error/timeout: fall back to MOCK_CARDS
// Expose approveCard(id), rejectCard(id), addComment(id, text) that call reducers
```
If `@clockworklabs/spacetimedb-sdk` has a different API, check their docs/types and adapt.
If SDK install fails (wrong package name), use raw WebSocket + SpacetimeDB binary protocol stub, or keep mock data fallback but make the hook *attempt* connection first.

After: `npm run build` must still pass with 0 errors.

### ✅ Item 4 — Wire live actions (approve/reject/comment) to reducers
**Agent: A (frontend)** — do after Item 3
- `useSpacetimeDB` should export `approveCard`, `rejectCard`, `addComment`
- Pass them via `Feed` props → `ActionCard` → `InteractionSidebar`
- When `isConnected=false` (mock mode): actions are local state only
- When `isConnected=true` (live): call reducer via SDK

### ✅ Item 5 — Create start.sh full-stack launcher
**Agent: Main**
Create `/Users/mkhare/Development/devtok/start.sh` — executable script that:
1. Starts SpacetimeDB server in background (with PATH including rustup)
2. Waits for it to be ready (`curl` health check loop)
3. Publishes the module (if not already published)
4. Seeds demo data (idempotent — reducer skips if agents exist)
5. Starts Python worker in background
6. Starts `npm run dev` in background
7. Prints URLs + PIDs
8. Traps EXIT/INT/TERM to kill all children

```bash
#!/usr/bin/env bash
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:$HOME/.local/bin:$PATH"
ROOT="$(cd "$(dirname "$0")" && pwd)"
# ... implementation
```
Make it `chmod +x` and test that it runs without error (even if SpacetimeDB already running).

### ✅ Item 6 — Create docs/plans design doc
**Agent: Main** — project rules require `docs/plans/YYYY-MM-DD-<topic>-design.md`
Create `/Users/mkhare/Development/devtok/docs/plans/2026-03-05-synapse-design.md`
Copy/condense from `design-log/001-synapse-architecture.md` — keep it concise.
Also create `docs/plans/` directory if it doesn't exist.

### ✅ Item 7 — Smoke test frontend
**Agent: A (frontend)**
```bash
cd /Users/mkhare/Development/devtok/frontend
npm run dev &
DEV_PID=$!
sleep 5
curl -s http://localhost:5173 | grep -q "root\|synapse\|Vite" && echo "✅ Frontend OK" || echo "❌ Frontend FAIL"
kill $DEV_PID
```
Frontend must respond on 5173.

### ✅ Item 8 — Paper.design artboards
**Agent: Main (requires Paper.design app open)**
Check if Paper is open: call `get_basic_info` from user-paper MCP.
If open: create 2 artboards:
  1. **Mobile ActionCard** (390x844) — full Cyber-Glass Dark Mode card with mesh bg, glass panel, sidebar, orbital dots
  2. **Desktop Feed** (1440x900) — wider layout showing 3-card peek
If Paper is NOT open: skip this item and mark as 🔶 BLOCKED (requires Paper.design app)

### ✅ Item 9 — Final commit + update MISSION.md
**Agent: Main**
```bash
cd /Users/mkhare/Development/devtok
git add -A
git commit -m "feat: Synapse 100% — live SpacetimeDB SDK, worker verified, start script, docs"
```
Then update this file — replace all ❌ with ✅ and add result notes.

---

## Key Commands Reference
```bash
# SpacetimeDB server (keep running)
spacetime start

# Check server health
curl -s http://localhost:3000/v1/database/synapse-backend-g9cee/sql \
  -H "Content-Type: text/plain" -d 'SELECT id FROM agent'

# Publish module (with rustup PATH)
export PATH="/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:$PATH"
cd /Users/mkhare/Development/devtok/backend/synapse-backend
spacetime publish synapse-backend-g9cee --server local

# Frontend dev
cd /Users/mkhare/Development/devtok/frontend && npm run dev

# Worker (inserts a card every 8–15s)
cd /Users/mkhare/Development/devtok/worker && python3 main.py

# Check live card count
curl -s -X POST http://localhost:3000/v1/database/synapse-backend-g9cee/sql \
  -H "Content-Type: text/plain" -d 'SELECT id, task_summary FROM action_card'
```

---

## Verification Checklist (run before claiming done)
```bash
# 1. Backend compiles
cd backend/synapse-backend && spacetime build 2>&1 | grep -q "successfully" && echo "✅ backend"

# 2. Frontend builds
cd frontend && npm run build 2>&1 | grep -q "built in" && echo "✅ frontend"

# 3. DB has data
curl -s -X POST http://localhost:3000/v1/database/synapse-backend-g9cee/sql \
  -H "Content-Type: text/plain" -d 'SELECT id FROM agent' | grep -q "rows" && echo "✅ db"

# 4. Worker can connect
cd worker && python3 -c "import httpx; r=httpx.get('http://localhost:3000'); print('✅ worker http' if r.status_code < 500 else '❌')"

# 5. Start script exists and is executable
test -x start.sh && echo "✅ start.sh"

# 6. Docs exist
test -f docs/plans/2026-03-05-synapse-design.md && echo "✅ docs"
```
