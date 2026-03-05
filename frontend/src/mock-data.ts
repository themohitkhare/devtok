import type { ActionCard } from './types';
import { TASK_TYPE_COLORS } from './design-tokens';

const codeDiffFixUseEffect = `
diff --git a/src/hooks/useUserProfile.ts b/src/hooks/useUserProfile.ts
index 4c3b1f2..8a7d9c1 100644
--- a/src/hooks/useUserProfile.ts
+++ b/src/hooks/useUserProfile.ts
@@ -12,11 +12,16 @@ export function useUserProfile(userId: string | null) {
   const [profile, setProfile] = useState<UserProfile | null>(null);
   const [isLoading, setIsLoading] = useState(false);
   const [error, setError] = useState<string | null>(null);

-  useEffect(() => {
-    if (!userId) return;
+  useEffect(() => {
+    if (!userId) {
+      setProfile(null);
+      return;
+    }

     let isCancelled = false;
     setIsLoading(true);
     setError(null);
@@ -30,7 +35,11 @@ export function useUserProfile(userId: string | null) {
         if (!isCancelled) {
           setError(err.message ?? 'Failed to load profile');
         }
-      });
+      });

-  }, []);
+  }, [userId, client]);

   return { profile, isLoading, error };
 }
`.trim();

const codeDiffAddStripeCustomerId = `
diff --git a/db/migrations/202503041210_add_stripe_customer_id.sql b/db/migrations/202503041210_add_stripe_customer_id.sql
new file mode 100644
index 0000000..c3f1ab2
--- /dev/null
+++ b/db/migrations/202503041210_add_stripe_customer_id.sql
@@ -0,0 +1,24 @@
+-- Add Stripe customer_id to users table
+ALTER TABLE users
+ADD COLUMN stripe_customer_id TEXT;
+
+CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_users_stripe_customer_id
+  ON users (stripe_customer_id)
+  WHERE stripe_customer_id IS NOT NULL;
+
+COMMENT ON COLUMN users.stripe_customer_id IS
+  'Foreign key to Stripe Customer.id used for billing integration';

diff --git a/db/schema.prisma b/db/schema.prisma
index 2a3b7d1..9d2f5c4 100644
--- a/db/schema.prisma
+++ b/db/schema.prisma
@@ -42,6 +42,9 @@ model User {
   email              String   @unique
   displayName        String?
   avatarUrl          String?
+  /// Stripe customer identifier for billing
+  stripeCustomerId   String?  @map("stripe_customer_id") @unique
+
   createdAt          DateTime @default(now())
   updatedAt          DateTime @updatedAt
 }
`.trim();

const terminalSecurityScan = `
[10:14:02] synapse-security-scanner starting npm audit...

> npm audit --json

found 3 vulnerabilities (3 high) in 1192 scanned packages
  run \`npm audit fix\` to fix 2 of them.

High            Arbitrary Code Execution
Package         minimist
Patched in      >=1.2.6
Dependency of   vite
Path            vite > rollup > minimist
More info       https://github.com/advisories/GHSA-7fhm-m57p-4q2v

High            Prototype Pollution
Package         lodash
Patched in      >=4.17.21
Dependency of   @vitejs/plugin-react
Path            @vitejs/plugin-react > lodash
More info       https://github.com/advisories/GHSA-jf85-cpcp-j695

High            Regular Expression Denial of Service
Package         braces
Patched in      >=2.3.2
Dependency of   tailwindcss
Path            tailwindcss > micromatch > braces
More info       https://github.com/advisories/GHSA-g95f-p29q-9xw4

[10:14:03] scan completed with HIGH severity issues
WARN 1 high severity advisory requires manual review
ERROR Failing pipeline: security gate [npm-audit-high-only] triggered
`.trim();

const terminalVitestSuccess = `
[11:02:15] synapse-test-runner

> pnpm vitest run --runInBand

 RUN  v0.34.6 /app

 ✓ api/agents/__tests__/agent-router.test.ts (12 tests)
 ✓ api/actions/__tests__/action-orchestrator.test.ts (8 tests)
 ✓ ui/components/__tests__/AgentProfileRing.test.tsx (9 tests)
 ✓ ui/components/__tests__/FeedKeyboardNav.test.tsx (6 tests)
 ✓ db/__tests__/billing-mapper.test.ts (12 tests)

 Test Files  5 passed (5)
      Tests  47 passed (47)
   Start at  11:02:15
   Duration  3.42s

 PASS All tests are green. Good job, agent.
`.trim();

const terminalDockerFailure = `
[12:44:01] synapse-deploy-orchestrator

> docker build -f ops/images/synapse.Dockerfile -t registry.local/synapse:sha-7f3c9d7 .

Sending build context to Docker daemon  71.42MB
Step 1/18 : FROM node:22-alpine AS base
 ---> 3b2c9e88c41b
Step 2/18 : WORKDIR /app
 ---> Using cache
 ---> 9a9c42a22f7d
Step 3/18 : COPY package.json pnpm-lock.yaml ./
 ---> Using cache
 ---> 1b9f314567dd
Step 4/18 : RUN corepack enable && pnpm install --frozen-lockfile
 ---> Running in 341afc1d92b4
Step 5/18 : COPY . .
 ---> 0f3bda831c10
Step 6/18 : RUN pnpm build
 ---> Running in 191c51dd6ab0

> synapse@0.1.0 build
> vite build

WARN  env var STRIPE_SECRET_KEY is not set, using test key
ERROR Failed to compile route /billing/webhook: missing STRIPE_WEBHOOK_SECRET
ERROR Build failed with 2 errors.
ERROR Command failed with exit code 1: pnpm build
FAIL  docker build exited with status 1
`.trim();

export const MOCK_CARDS: ActionCard[] = [
  {
    id: 'card-frontend-ui-agent',
    agentName: 'Frontend UI Agent',
    agentHandle: '@frontend-ui-agent',
    specialty: 'React surface stabilization and ergonomics',
    status: 'running',
    visualType: 'CodeDiff',
    taskSummary: 'Fix unstable React useEffect dependencies in user profile hook',
    content: codeDiffFixUseEffect,
    priority: 100,
    concurrentTasks: [
      {
        id: 't-code',
        taskType: 'code',
        status: 'running',
        color: TASK_TYPE_COLORS.code,
      },
      {
        id: 't-test',
        taskType: 'test',
        status: 'running',
        color: TASK_TYPE_COLORS.test,
      },
      {
        id: 't-review',
        taskType: 'review',
        status: 'running',
        color: TASK_TYPE_COLORS.review,
      },
    ],
    tags: ['react', 'hooks', 'stability', 'deps'],
  },
  {
    id: 'card-db-migration',
    agentName: 'DB Migration Agent',
    agentHandle: '@db-migration-agent',
    specialty: 'Online schema changes for billing',
    status: 'thinking',
    visualType: 'CodeDiff',
    taskSummary: 'Add Stripe customer_id to users table with concurrent index',
    content: codeDiffAddStripeCustomerId,
    priority: 95,
    concurrentTasks: [
      {
        id: 't-migrate',
        taskType: 'migrate',
        status: 'running',
        color: TASK_TYPE_COLORS.migrate,
      },
      {
        id: 't-test',
        taskType: 'test',
        status: 'running',
        color: TASK_TYPE_COLORS.test,
      },
    ],
    tags: ['postgres', 'stripe', 'migration', 'zero-downtime'],
  },
  {
    id: 'card-security-scan',
    agentName: 'Security Scanner',
    agentHandle: '@security-scanner',
    specialty: 'Dependency and surface security scanning',
    status: 'blocked',
    visualType: 'TerminalOutput',
    taskSummary: 'npm audit reported 3 high severity vulnerabilities',
    content: terminalSecurityScan,
    priority: 90,
    concurrentTasks: [
      {
        id: 't-scan',
        taskType: 'scan',
        status: 'failed',
        color: TASK_TYPE_COLORS.scan,
      },
    ],
    tags: ['security', 'npm', 'audit', 'high-risk'],
  },
  {
    id: 'card-test-runner',
    agentName: 'Test Runner',
    agentHandle: '@test-runner',
    specialty: 'Continuous regression protection',
    status: 'success',
    visualType: 'TerminalOutput',
    taskSummary: 'Vitest suite finished 47/47 tests passing',
    content: terminalVitestSuccess,
    priority: 80,
    concurrentTasks: [],
    tags: ['vitest', 'ci', 'green', 'regression-safety'],
  },
  {
    id: 'card-deploy-orchestrator',
    agentName: 'Deploy Orchestrator',
    agentHandle: '@deploy-orchestrator',
    specialty: 'Blue/green deploys for Synapse',
    status: 'failed',
    visualType: 'TerminalOutput',
    taskSummary: 'Docker build failed due to missing STRIPE_WEBHOOK_SECRET',
    content: terminalDockerFailure,
    priority: 70,
    concurrentTasks: [
      {
        id: 't-deploy',
        taskType: 'deploy',
        status: 'failed',
        color: TASK_TYPE_COLORS.deploy,
      },
    ],
    tags: ['deploy', 'docker', 'stripe', 'env-vars'],
  },
];
