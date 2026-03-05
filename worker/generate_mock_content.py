"""
Mock content for Synapse action cards: code diffs, terminal output, and task summaries.
"""

from typing import Literal

VisualType = Literal["CodeDiff", "TerminalOutput", "StatusUpdate"]

MOCK_CODE_DIFFS = [
    # 1. Adding Stripe customer ID to user schema
    """```diff
--- a/src/db/schema.ts
+++ b/src/db/schema.ts
@@ -12,6 +12,7 @@ export interface User {
   email: string;
   name: string;
   role: "admin" | "user";
+  stripe_customer_id?: string;
   created_at: number;
   updated_at: number;
 }
```""",
    # 2. Fixing React hook dependency array
    """```diff
--- a/src/hooks/useProject.ts
+++ b/src/hooks/useProject.ts
@@ -18,7 +18,7 @@ export function useProject(projectId: string | null) {
       const data = await fetchProject(projectId);
       setProject(data);
     }
-  }, [projectId]);
+  }, [projectId, fetchProject]);
   return { project, loading, error };
 }
```""",
    # 3. Adding retry logic to API client
    """```diff
--- a/src/api/client.ts
+++ b/src/api/client.ts
@@ -22,6 +22,14 @@ async function request<T>(url: string, opts?: RequestInit): Promise<T> {
       throw new Error(\`HTTP \${res.status}\`);
     }
     return res.json();
+  } catch (e) {
+    for (let i = 0; i < 3; i++) {
+      await new Promise(r => setTimeout(r, 1000 * (i + 1)));
+      try { return await request(url, opts); } catch (_) {}
+    }
+    throw e;
+  }
   }
```""",
    # 4. Optimizing database query with index
    """```diff
--- a/migrations/20250305_add_index.sql
+++ b/migrations/20250305_add_index.sql
@@ -0,0 +1,4 @@
+-- Index for action_cards by agent and created_at
+CREATE INDEX CONCURRENTLY idx_action_cards_agent_created
+ON action_cards (agent_id, created_at DESC);
```""",
    # 5. Updating Tailwind configuration
    """```diff
--- a/tailwind.config.js
+++ b/tailwind.config.js
@@ -8,6 +8,9 @@ export default {
       colors: {
         primary: { DEFAULT: "#0ea5e9", dark: "#0284c7" },
         surface: "#f8fafc",
+        synapse: {
+          card: "#f1f5f9",
+          accent: "#6366f1",
+        },
       },
     },
   },
```""",
]

MOCK_TERMINAL_OUTPUTS = [
    # 1. npm install
    """```text
$ npm install
added 42 packages, and audited 312 packages in 4s
42 packages are looking for funding
  run \`npm fund\` for details
found 0 vulnerabilities
```""",
    # 2. vitest run
    """```text
$ npx vitest run
 ✓ src/utils/format.test.ts (3)
 ✓ src/api/client.test.ts (5)
 ✓ src/hooks/useProject.test.ts (4)
 Test Files  3 passed (3)
      Tests  12 passed (12)
   Start at 14:32:01
   Duration 1.24s
```""",
    # 3. cargo build
    """```text
$ cargo build --release
   Compiling synapse-module v0.1.0
   Compiling spacetimedb-sdk v0.5.0
   Compiling tokio v1.35
   Compiling synapse-module v0.1.0
    Finished release [optimized] target(s) in 18.42s
```""",
    # 4. git operations
    """```text
$ git fetch origin
$ git merge origin/main --no-edit
Updating a1b2c3d..e4f5g6h
Fast-forward
 src/components/Card.tsx | 12 ++++++------
 1 file changed, 6 insertions(+), 6 deletions(-)
$ git push origin feature/synapse-cards
```""",
    # 5. docker build
    """```text
$ docker build -t synapse-worker:latest .
[1/4] FROM node:20-alpine
[2/4] COPY package*.json ./
[3/4] RUN npm ci --omit=dev
[4/4] COPY . .
exporting to image
done 12.4s
```""",
]

MOCK_TASK_SUMMARIES: dict[VisualType, list[str]] = {
    "CodeDiff": [
        "Add Stripe customer ID to user schema",
        "Fix React hook dependency array in useProject",
        "Add retry logic to API client",
        "Add index on action_cards (agent_id, created_at)",
        "Add synapse theme colors to Tailwind config",
    ],
    "TerminalOutput": [
        "Install dependencies (npm install)",
        "Run test suite (vitest)",
        "Build release binary (cargo build)",
        "Sync branch with main and push",
        "Build Docker image for worker",
    ],
    "StatusUpdate": [
        "Task queued",
        "Running checks",
        "Deploy in progress",
        "Review requested",
    ],
}
