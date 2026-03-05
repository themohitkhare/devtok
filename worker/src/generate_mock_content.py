import random


MOCK_CODE_DIFFS = [
    """
diff --git a/src/user.ts b/src/user.ts
index a1b2c3d..d4e5f6a 100644
--- a/src/user.ts
+++ b/src/user.ts
@@ -1,10 +1,18 @@
 export type User = {
   id: string;
   email: string;
   name: string;
+  /**
+   * Stripe customer id used for billing.
+   * Nullable until the user completes checkout.
+   */
+  stripeCustomerId?: string | null;
 };

 export type UserWithMeta = User & {
   createdAt: string;
   updatedAt: string;
 };

diff --git a/prisma/schema.prisma b/prisma/schema.prisma
index 1111111..2222222 100644
--- a/prisma/schema.prisma
+++ b/prisma/schema.prisma
@@ -15,11 +15,18 @@ model User {
   id           String   @id @default(cuid())
   email        String   @unique
   name         String?
   createdAt    DateTime @default(now())
   updatedAt    DateTime @updatedAt
+
+  /// Stripe customer id for billing.
+  /// Nullable so existing users don't break.
+  stripeCustomerId String? @db.VarChar(255)
 }

@@ -40,6 +47,10 @@ model Subscription {
   user        User     @relation(fields: [userId], references: [id])
   userId      String
   status      String
   plan        String
   createdAt   DateTime @default(now())
 }
    """,
    """
diff --git a/src/components/AuthForm.tsx b/src/components/AuthForm.tsx
index 9abc123..def4567 100644
--- a/src/components/AuthForm.tsx
+++ b/src/components/AuthForm.tsx
@@ -32,15 +32,17 @@ export const AuthForm: React.FC<AuthFormProps> = ({ mode = "signin", onSuccess }
   const [email, setEmail] = useState("");
   const [password, setPassword] = useState("");
   const [loading, setLoading] = useState(false);

-  const handleSubmit = useCallback(
-    async (e: React.FormEvent) => {
-      e.preventDefault();
-      setLoading(true);
-      await onSubmit({ mode, email, password });
-      setLoading(false);
-    },
-    [email, password]
-  );
+  const handleSubmit = useCallback(
+    async (e: React.FormEvent) => {
+      e.preventDefault();
+      setLoading(true);
+      await onSubmit({ mode, email, password });
+      setLoading(false);
+    },
+    // include mode and onSubmit so we don't capture stale values
+    [mode, email, password, onSubmit]
+  );

   return (
     <form onSubmit={handleSubmit} className="space-y-4">
@@ -71,7 +73,7 @@ export const AuthForm: React.FC<AuthFormProps> = ({ mode = \"signin\", onSuccess }
         </button>
       </div>
     </form>
   );
 }
    """,
    """
diff --git a/src/lib/apiClient.ts b/src/lib/apiClient.ts
index 1234abc..5678def 100644
--- a/src/lib/apiClient.ts
+++ b/src/lib/apiClient.ts
@@ -1,21 +1,57 @@
 import type { RequestInit } from "node-fetch";

 const BASE_URL = process.env.API_BASE_URL ?? "https://api.example.com";

-export async function request(path: string, init: RequestInit = {}) {
-  const res = await fetch(BASE_URL + path, {
-    ...init,
-    headers: {
-      "Content-Type": "application/json",
-      ...(init.headers || {}),
-    },
-  });
-
-  if (!res.ok) {
-    throw new Error(`Request failed with status ${res.status}`);
-  }
-
-  return res.json();
-}
+const MAX_RETRIES = 4;
+const INITIAL_DELAY_MS = 250;
+
+function sleep(ms: number) {
+  return new Promise((resolve) => setTimeout(resolve, ms));
+}
+
+export async function request(path: string, init: RequestInit = {}) {
+  let attempt = 0;
+  let delay = INITIAL_DELAY_MS;
+
+  // simple exponential backoff with full jitter
+  while (attempt <= MAX_RETRIES) {
+    const res = await fetch(BASE_URL + path, {
+      ...init,
+      headers: {
+        "Content-Type": "application/json",
+        ...(init.headers || {}),
+      },
+    });
+
+    if (res.ok) {
+      return res.json();
+    }
+
+    const isRetryable = res.status >= 500 || res.status === 429;
+
+    if (!isRetryable || attempt === MAX_RETRIES) {
+      const body = await res.text().catch(() => "<unreadable body>");
+      throw new Error(
+        `Request failed with status ${res.status} after ${attempt + 1} attempts: ${body}`,
+      );
+    }
+
+    const jitter = Math.random() * delay;
+    await sleep(delay + jitter);
+
+    attempt += 1;
+    delay *= 2;
+  }
+}
    """,
    """
diff --git a/prisma/migrations/20250305120000_add_user_email_idx/migration.sql b/prisma/migrations/20250305120000_add_user_email_idx/migration.sql
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/prisma/migrations/20250305120000_add_user_email_idx/migration.sql
@@ -0,0 +1,18 @@
+-- Improve performance of user lookups by email for auth flows.
+-- Before adding index, the query plan was doing a sequential scan
+-- on \"User\" for ~150k rows.
+
+CREATE INDEX CONCURRENTLY IF NOT EXISTS \"User_email_idx\"
+  ON \"User\" (\"email\");
+
+-- Verify with:
+-- EXPLAIN ANALYZE
+-- SELECT * FROM \"User\" WHERE \"email\" = 'demo@example.com';
    """,
    """
diff --git a/tailwind.config.cjs b/tailwind.config.cjs
index 13579bd..2468ace 100644
--- a/tailwind.config.cjs
+++ b/tailwind.config.cjs
@@ -4,10 +4,29 @@ /** @type {import('tailwindcss').Config} */
 module.exports = {
   content: ["./index.html", "./src/**/*.{ts,tsx,js,jsx}"],
   theme: {
-    extend: {},
+    extend: {
+      colors: {
+        synapse: {
+          50: "#f4f7ff",
+          100: "#e4ebff",
+          200: "#c3d0ff",
+          300: "#9aaeff",
+          400: "#647fff",
+          500: "#3b5aff",
+          600: "#2440db",
+          700: "#1a31aa",
+          800: "#162884",
+          900: "#121f63",
+        },
+      },
+    },
   },
   plugins: [],
 };
    """,
]


MOCK_TERMINAL_OUTPUTS = [
    """$ npm install framer-motion lucide-react

added 22 packages, and audited 964 packages in 4s

9 packages are looking for funding
  run `npm fund` for details

found 0 vulnerabilities

vite v5.1.0 dev server running at:

> Local:   http://localhost:5173/
> Network: use `--host` to expose
""",
    """$ pnpm vitest run

 DEV  v1.6.3 /Users/dev/app

 ✓ src/components/AuthForm.test.tsx (6)
 ✓ src/components/Layout.test.tsx (3)
 ✓ src/hooks/useAuthToken.test.ts (4)
 ✓ src/lib/apiClient.test.ts (8)
 ✓ src/pages/dashboard/Dashboard.test.tsx (12)
 ✓ src/pages/projects/ProjectList.test.tsx (9)
 ✓ src/utils/formatters.test.ts (5)

 Test Files  7 passed (7)
      Tests  47 passed (47)
   Start at  11:03:21
   Duration  2.94s (transform 328ms, setup 421ms, collect 412ms, tests 1.1s)
""",
    """$ cargo build -p synapse-backend
   Compiling synapse-core v0.1.0 (/Users/dev/devtok/synapse/core)
   Compiling synapse-db v0.1.0 (/Users/dev/devtok/synapse/db)
   Compiling synapse-backend v0.1.0 (/Users/dev/devtok/synapse/backend)
    Finished dev [unoptimized + debuginfo] target(s) in 24.91s

 $ target/debug/synapse-backend
 2025-03-05T11:12:58.104Z  INFO synapse_backend: starting HTTP server at 0.0.0.0:8080
""",
    """$ git fetch origin
From github.com:devtok/synapse
 * [new branch]      feature/agent-ui -> origin/feature/agent-ui

$ git rebase origin/main
Successfully rebased and updated refs/heads/feature/agent-ui.

$ git push origin feature/agent-ui
Enumerating objects: 18, done.
Counting objects: 100% (18/18), done.
Delta compression using up to 8 threads
Compressing objects: 100% (11/11), done.
Writing objects: 100% (12/12), 2.13 KiB | 2.13 MiB/s, done.
Total 12 (delta 7), reused 0 (delta 0), pack-reused 0
To github.com:devtok/synapse.git
   82e5b81..b93bdf0  feature/agent-ui -> feature/agent-ui
""",
    """$ docker build -t synapse-worker .
[+] Building 27.4s (10/13)
 => [internal] load build definition from Dockerfile                   0.1s
 => [internal] load .dockerignore                                      0.0s
 => [internal] load metadata for docker.io/library/node:20-alpine      1.9s
 => [1/8] FROM docker.io/library/node:20-alpine@sha256:...             0.0s
 => [2/8] WORKDIR /app                                                 0.1s
 => [3/8] COPY package.json pnpm-lock.yaml ./                          0.1s
 => [4/8] RUN corepack enable && pnpm install --frozen-lockfile       18.3s
 => [5/8] COPY . .                                                     0.2s
 => ERROR [6/8] RUN pnpm build                                         6.5s
------
> [6/8] RUN pnpm build:
> > synapse-web@0.1.0 build /app
> > vite build
>
> failed to load config from /app/vite.config.ts
> error when starting dev server:
> Error [ERR_MODULE_NOT_FOUND]: Cannot find module '/app/node_modules/@synapse/core'
>   imported from /app/vite.config.ts
>     at new NodeError (node:internal/errors:405:5)
>     at finalizeResolution (node:internal/modules/esm/resolve:327:11)
>     at moduleResolve (node:internal/modules/esm/resolve:980:10)
>     at defaultResolve (node:internal/modules/esm/resolve:1193:11)
>     at nextResolve (node:internal/modules/esm/hooks:864:28)
>     at Hooks.resolve (node:internal/modules/esm/hooks:302:30)
>     at ModuleLoader.defaultResolve (node:internal/modules/esm/loader:367:35)
>     at ModuleLoader.resolve (node:internal/modules/esm/loader:336:27)
>     at ModuleLoader.getModuleJob (node:internal/modules/esm/loader:248:18)
>     at ModuleLoader.import (node:internal/modules/esm/loader:335:22) {
>   code: 'ERR_MODULE_NOT_FOUND'
> }
------
Docker build failed: exit code 1
""",
]


MOCK_TASK_SUMMARIES = {
    "CodeDiff": [
        "Add Stripe customer id to user domain type and Prisma schema for billing.",
        "Fix AuthForm useCallback dependency array to avoid stale mode and handler.",
        "Introduce exponential backoff with jitter to API client request helper.",
        "Add PostgreSQL index to speed up User email lookups in auth flows.",
        "Extend Tailwind config with custom synapse color palette.",
    ],
    "TerminalOutput": [
        "npm install adds framer-motion and lucide-react with a clean audit.",
        "Vitest test run with 47 tests all passing in under 3 seconds.",
        "cargo build successfully compiles synapse-backend and starts HTTP server.",
        "git fetch, rebase against origin/main, and push feature branch successfully.",
        "docker build fails during pnpm build due to missing @synapse/core module.",
    ],
}


def get_random_diff() -> str:
    return random.choice(MOCK_CODE_DIFFS)


def get_random_terminal() -> str:
    return random.choice(MOCK_TERMINAL_OUTPUTS)


def get_random_summary(visual_type: str) -> str:
    if visual_type not in MOCK_TASK_SUMMARIES:
        raise ValueError(f"Unknown visual_type: {visual_type}")
    return random.choice(MOCK_TASK_SUMMARIES[visual_type])

