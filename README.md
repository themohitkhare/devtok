# Synapse

> TikTok-style interface for monitoring and approving local AI agent tasks.

Swipe through ActionCards (agent reels). Double-tap to approve. Comment to redirect. Escalate to a human.

```
┌─────────────────────────────────┐
│  @frontend-ui-agent             │  ●●● orbital task lights
│                                 │  ✓ approve
│  ┌────────────────────────┐     │  💬 comment
│  │ - old code line (red)  │     │  ⌨  terminal
│  │ + new code line (green)│     │  ⚠  escalate
│  │   context line         │     │
│  └────────────────────────┘     │
│                                 │
│  @frontend-ui-agent   ● Running │
│  Fix React useEffect deps       │
│  #react #typescript             │
└─────────────────────────────────┘
       ↕ swipe to navigate
```

## Tech Stack

| Layer | Tech |
|-------|------|
| Database + State | SpacetimeDB (Rust module, local) |
| Frontend | React 19 + TypeScript + Vite + Tailwind v4 + Framer Motion |
| Agent Worker | Python 3 + httpx |

## Project Structure

```
devtok/
├── backend/synapse-backend/   # SpacetimeDB Rust module
│   └── spacetimedb/src/lib.rs # 5 tables + 10 reducers
├── frontend/                  # React/Vite app
│   └── src/
│       ├── components/        # Feed, ActionCard, UI components
│       ├── hooks/             # useSpacetimeDB
│       ├── mock-data.ts       # 5 realistic demo cards
│       └── types.ts           # TypeScript interfaces
├── worker/                    # Python mock agent
│   ├── main.py
│   └── generate_mock_content.py
└── design-log/                # Architecture decisions
```

## Quick Start

### 1. Frontend (development with mock data)
```bash
cd frontend
npm install
npm run dev
# → http://localhost:5173
```

### 2. Start SpacetimeDB (for live data)
```bash
# Install spacetime CLI (already done if backend was set up)
spacetime start

# In another terminal, publish the module:
cd backend/synapse-backend
spacetime publish synapse --server local
```

### 3. Run the mock agent worker
```bash
cd worker
pip install -r requirements.txt
python main.py
# Pushes a new ActionCard to SpacetimeDB every 8–15 seconds
```

## Interactions

| Gesture | Action |
|---------|--------|
| Double-tap anywhere | Approve card (`approve_action` reducer) |
| Tap ✓ button | Approve card |
| Tap 💬 button | Open comment overlay |
| Tap ⚠ button | Escalate to human |
| Swipe up/down | Next/previous card |
| Arrow keys | Next/previous card (desktop) |

## Visual Design

**Cyber-Glass Dark Mode** — `#0a0e1a` background, glassmorphism content panels, orbital task lights on agent avatars.

Agent avatar orbital lights: each dot = one concurrent task. Colors encode task type (blue=code, green=test, purple=deploy...). Active tasks blink.

## SpacetimeDB Schema

Tables: `project`, `agent`, `action_card`, `feedback`, `concurrent_task`

Reducers: `create_agent`, `insert_action_card`, `approve_action`, `reject_action`, `add_comment`, `escalate_action`, `update_agent_status`, `insert_concurrent_task`, `complete_task`, `seed_demo_data`
