export type DeployState = "healthy" | "deploying" | "degraded" | "drift" | "failed";

export type Deployment = {
  id: string;
  name: string;
  provider: "vercel" | "aws" | "fly" | "railway" | "cloudflare" | "supabase";
  providerLabel: string;
  region: string;
  url: string;
  branch: string;
  state: DeployState;
  lastDeploy: string;
  lastCommit: string;
  lastDeployer: string;
  workspaces: { id: string; name: string; role: string }[];
  envCount: number;
  integrations: number;
  metrics: { rps: string; p95: string; error: string };
  alert?: string;
};

export const DEPLOYMENTS: Deployment[] = [
  {
    id: "d-prod-web",
    name: "puffer-web · production",
    provider: "vercel",
    providerLabel: "Vercel",
    region: "iad1 · us-east",
    url: "puffer.app",
    branch: "main",
    state: "healthy",
    lastDeploy: "3m ago",
    lastCommit: "a91c4e2 · feat: passkeys",
    lastDeployer: "Flint",
    workspaces: [
      { id: "puffer-web", name: "puffer-web", role: "frontend" },
      { id: "puffer-marketing", name: "puffer-marketing", role: "marketing" }
    ],
    envCount: 24,
    integrations: 5,
    metrics: { rps: "840", p95: "182ms", error: "0.04%" }
  },
  {
    id: "d-stg-web",
    name: "puffer-web · staging",
    provider: "vercel",
    providerLabel: "Vercel",
    region: "iad1 · us-east",
    url: "staging.puffer.app",
    branch: "develop",
    state: "deploying",
    lastDeploy: "just now",
    lastCommit: "7b3d021 · fix: dark-mode flicker",
    lastDeployer: "Flint",
    workspaces: [{ id: "puffer-web", name: "puffer-web", role: "frontend" }],
    envCount: 22,
    integrations: 5,
    metrics: { rps: "12", p95: "156ms", error: "0.00%" }
  },
  {
    id: "d-prod-api",
    name: "stripe-api · production",
    provider: "aws",
    providerLabel: "AWS · ECS Fargate",
    region: "us-east-1",
    url: "api.puffer.app",
    branch: "main",
    state: "degraded",
    lastDeploy: "2h ago",
    lastCommit: "f02ae81 · chore: bump node",
    lastDeployer: "Sage",
    workspaces: [
      { id: "stripe-api", name: "stripe-api", role: "api" },
      { id: "billing-worker", name: "billing-worker", role: "worker" }
    ],
    envCount: 41,
    integrations: 8,
    metrics: { rps: "2.1k", p95: "480ms", error: "1.3%" },
    alert: "p95 latency 2× baseline · webhook queue backing up"
  },
  {
    id: "d-infra",
    name: "infra · shared",
    provider: "aws",
    providerLabel: "AWS · Terraform",
    region: "us-east-1",
    url: "—",
    branch: "main",
    state: "drift",
    lastDeploy: "1d ago",
    lastCommit: "c4e71a0 · chore: tf bump",
    lastDeployer: "Willow",
    workspaces: [{ id: "infra-tf", name: "infra-tf", role: "infrastructure" }],
    envCount: 18,
    integrations: 3,
    metrics: { rps: "—", p95: "—", error: "—" },
    alert: "Planned drift: 3 resources out of sync"
  },
  {
    id: "d-edge",
    name: "edge-cdn",
    provider: "cloudflare",
    providerLabel: "Cloudflare Workers",
    region: "global · 270 pops",
    url: "edge.puffer.app",
    branch: "main",
    state: "healthy",
    lastDeploy: "18h ago",
    lastCommit: "2a1bb40 · perf: cache signed urls",
    lastDeployer: "Juno",
    workspaces: [{ id: "puffer-edge", name: "puffer-edge", role: "edge" }],
    envCount: 9,
    integrations: 2,
    metrics: { rps: "18k", p95: "14ms", error: "0.01%" }
  },
  {
    id: "d-db",
    name: "primary-db",
    provider: "supabase",
    providerLabel: "Supabase · Postgres 15",
    region: "us-east-1",
    url: "db.puffer.app",
    branch: "—",
    state: "healthy",
    lastDeploy: "3d ago",
    lastCommit: "migrations @ 0142",
    lastDeployer: "Sage",
    workspaces: [
      { id: "stripe-api", name: "stripe-api", role: "api" },
      { id: "puffer-web", name: "puffer-web", role: "frontend" },
      { id: "billing-worker", name: "billing-worker", role: "worker" }
    ],
    envCount: 6,
    integrations: 4,
    metrics: { rps: "4.2k qps", p95: "3ms", error: "0.00%" }
  }
];

export type Secret = { key: string; preview: string; scope: "runtime" | "build"; updated: string; by: string; rotate?: boolean };
export const SECRETS: Record<string, Secret[]> = {
  "d-prod-api": [
    { key: "DATABASE_URL",          preview: "postgres://…@db.puffer.app/prod", scope: "runtime", updated: "12d ago", by: "Sage" },
    { key: "STRIPE_SECRET_KEY",     preview: "sk_live_51Mn…",                   scope: "runtime", updated: "3mo ago", by: "harvey" },
    { key: "STRIPE_WEBHOOK_SECRET", preview: "whsec_8a0…",                      scope: "runtime", updated: "3mo ago", by: "harvey" },
    { key: "REDIS_URL",             preview: "rediss://…@cache.puffer.app",     scope: "runtime", updated: "2w ago",  by: "Willow" },
    { key: "SENTRY_DSN",            preview: "https://…ingest.sentry.io/…",     scope: "runtime", updated: "2mo ago", by: "harvey" },
    { key: "OPENAI_API_KEY",        preview: "sk-proj-BA8…",                    scope: "runtime", updated: "1mo ago", by: "lin" },
    { key: "JWT_SIGNING_KEY",       preview: "—",                               scope: "runtime", updated: "6mo ago", by: "harvey", rotate: true },
    { key: "AWS_ACCESS_KEY_ID",     preview: "AKIA2…XJ4F",                      scope: "build",   updated: "2mo ago", by: "Willow" }
  ]
};

export type Integration = { kind: string; name: string; note: string; status: "connected" | "degraded" };
export const INTEGRATIONS: Record<string, Integration[]> = {
  "d-prod-api": [
    { kind: "postgres", name: "primary-db",    note: "Supabase · us-east-1",     status: "connected" },
    { kind: "redis",    name: "session-cache", note: "Upstash · global",         status: "connected" },
    { kind: "stripe",   name: "Stripe",        note: "live key · acct_xYm",      status: "connected" },
    { kind: "sentry",   name: "Sentry",        note: "puffer / stripe-api",      status: "connected" },
    { kind: "github",   name: "GitHub",        note: "puffer/stripe-api · main", status: "connected" },
    { kind: "slack",    name: "Slack alerts",  note: "#ops-api",                 status: "connected" },
    { kind: "s3",       name: "invoices-s3",   note: "s3://puffer-invoices",     status: "connected" },
    { kind: "openai",   name: "OpenAI",        note: "org-puffer",               status: "degraded" }
  ]
};

export type MemoryItem = {
  id: string;
  kind: "incident" | "runbook" | "fact" | "pitfall" | "convention";
  title: string;
  body: string;
  source: { kind: string; ref: string };
  confidence: "high" | "medium" | "low";
  savedBy: string;
  time: string;
  tags: string[];
  uses: number;
};

export const MEMORY: Record<string, MemoryItem[]> = {
  "d-prod-api": [
    { id: "m1", kind: "incident",   title: "Node 20 drops http-keepalive by default", body: "After bumping node 18 → 20 on 2024-11-21, p95 on /subscription/update doubled. Fix: set agent.keepAlive=true in lib/http.ts. Applies to any service using the same http client.", source: { kind: "deploy",  ref: "f02ae81" },           confidence: "high",   savedBy: "Sage",   time: "12h ago", tags: ["runtime", "performance", "node-20"], uses: 4 },
    { id: "m2", kind: "runbook",    title: "Stripe webhook backlog recovery",          body: "When billing-invoices queue lag > 1s, drain manually with scripts/drain-webhooks.ts --since=30m. Safe to re-run; idempotency enforced by event_id.",                                    source: { kind: "incident", ref: "INC-214 · 2024-09-04" }, confidence: "high",   savedBy: "harvey", time: "3d ago",  tags: ["stripe", "queue", "runbook"],        uses: 2 },
    { id: "m3", kind: "fact",       title: "DB pool cap is 50, not 100",                body: "RDS t4g.large maxes at 50 connections despite docs. Pool logs warn at 48/50. Keep pgbouncer pool_size ≤ 40 to leave headroom for migrations.",                                          source: { kind: "logs",     ref: "api · 2024-11-18" },     confidence: "high",   savedBy: "Willow", time: "6d ago",  tags: ["postgres", "capacity"],              uses: 7 },
    { id: "m4", kind: "pitfall",    title: "Cron drift in us-east-1a",                  body: "CloudWatch scheduled rules skew ~90s in az 1a during maintenance windows. If a cron fires late, don't alert — grace period is 3 minutes.",                                              source: { kind: "deploy",   ref: "6f8c120" },              confidence: "medium", savedBy: "Sage",   time: "2w ago",  tags: ["cron", "aws", "alerts"],             uses: 1 },
    { id: "m5", kind: "convention", title: "Feature flags: off-by-default in prod",     body: "Any flag that ships dark here should default to false in prod and true in staging. Override via LAUNCHDARKLY_PROD_DEFAULT secret — do not flip in code.",                                   source: { kind: "pr",       ref: "#4217" },                confidence: "high",   savedBy: "lin",    time: "1mo ago", tags: ["feature-flags", "rollout"],          uses: 12 },
    { id: "m6", kind: "pitfall",    title: "Don't redeploy during Stripe reconciliation window", body: "03:00–03:15 UTC daily, the reconciliation job holds a row-level lock on subscriptions. Deploys that touch the subscriptions table will stall ECS health checks.",                 source: { kind: "incident", ref: "INC-198" },              confidence: "high",   savedBy: "Sage",   time: "2mo ago", tags: ["stripe", "deploy-window"],           uses: 5 }
  ]
};

export const KIND_META: Record<string, { label: string; icon: string; color: string }> = {
  incident:   { label: "Incident",   icon: "flame",  color: "oklch(0.55 0.18 25)"  },
  runbook:    { label: "Runbook",    icon: "wrench", color: "oklch(0.5 0.15 240)"  },
  fact:       { label: "Fact",       icon: "bolt",   color: "oklch(0.55 0.14 200)" },
  pitfall:    { label: "Pitfall",    icon: "bug",    color: "oklch(0.58 0.17 55)"  },
  convention: { label: "Convention", icon: "shield", color: "oklch(0.5 0.12 290)"  }
};

export type DeployHistoryItem = {
  id: string;
  commit: string;
  branch: string;
  deployer: string;
  state: DeployState;
  time: string;
  dur: string;
  current?: boolean;
};

export function historyFor(d: Deployment): DeployHistoryItem[] {
  return [
    { id: "b-1428", commit: d.lastCommit, branch: d.branch, deployer: d.lastDeployer, state: d.state === "deploying" ? "deploying" : "healthy", time: d.lastDeploy, dur: "1m 48s", current: true },
    { id: "b-1427", commit: "6f8c120 · chore: bump node 20.11", branch: "main", deployer: "Sage",   state: "healthy", time: "2h ago",  dur: "1m 52s" },
    { id: "b-1426", commit: "a012de5 · fix: retry queue backoff", branch: "main", deployer: "Sage",   state: "healthy", time: "6h ago",  dur: "2m 04s" },
    { id: "b-1425", commit: "e3c9a71 · feat: seat-based pricing", branch: "main", deployer: "Flint",  state: "failed",  time: "1d ago",  dur: "0m 41s" },
    { id: "b-1424", commit: "90d1f32 · revert: seat pricing",     branch: "main", deployer: "Flint",  state: "healthy", time: "1d ago",  dur: "1m 33s" }
  ];
}
