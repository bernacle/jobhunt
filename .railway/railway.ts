// JobHunt Cloud on Railway (Infrastructure as Code).
//
// One image (the repository's Dockerfile), four resources:
//
//   Postgres              the database (shared corpus + private data)
//   api                   `jobhunt server`: HTTP API and hosted MCP; runs
//                         `jobhunt migrate` before each deploy
//   worker-discovery      cron: reads the sources that are due
//   worker-verification   cron: re-verifies the jobs that matter
//
// Secrets are declared with preserve(): set them once in the dashboard or
// with `railway variable set KEY=… -s api` (and seal them); applying this
// file never overwrites or deletes them. See README.md, "JobHunt Cloud on
// Railway", for every variable and the first deploy.
//
//   npm install            # the `railway` package (package.json)
//   railway link           # the project and environment
//   railway config plan    # review
//   railway config apply

import { defineRailway, github, postgres, preserve, project, service } from "railway/iac";

export default defineRailway((ctx) => {
  const db = postgres("Postgres");
  const source = () => github("bernacle/jobhunt", { branch: "main" });
  const build = { builder: "DOCKERFILE" as const, dockerfilePath: "Dockerfile" };

  const api = service("api", {
    source: source(),
    build,
    start: "jobhunt server",
    // Migrations run once per deploy, before the new version starts;
    // concurrent runs are serialized by a Postgres advisory lock.
    preDeploy: "jobhunt migrate",
    // Deploy-time check: the process answers and Postgres is reachable with
    // the current schema. (Railway does not keep polling it afterwards.)
    healthcheck: "/ready",
    healthcheckTimeout: 120,
    env: {
      DATABASE_URL: db.env.DATABASE_URL,
      JOBHUNT_DB_MAX_CONNECTIONS: "10",
      // Secrets and per-environment values, set by hand:
      JOBHUNT_ENCRYPTION_KEYS: preserve(),
      JOBHUNT_PUBLIC_URL: preserve(),
      JOBHUNT_OIDC_ISSUER: preserve(),
      JOBHUNT_OIDC_AUDIENCE: preserve(),
      JOBHUNT_OIDC_CLI_CLIENT_ID: preserve(),
      JOBHUNT_ALLOWED_ORIGINS: preserve(),
    },
    deploy: {
      restartPolicyType: "ON_FAILURE",
      restartPolicyMaxRetries: 10,
      // SIGTERM, then this long to finish requests in flight.
      drainingSeconds: 30,
      overlapSeconds: 20,
    },
  });

  const discovery = service("worker-discovery", {
    source: source(),
    build,
    start: "jobhunt worker discovery",
    env: {
      DATABASE_URL: db.env.DATABASE_URL,
      JOBHUNT_DB_MAX_CONNECTIONS: "4",
    },
    deploy: {
      // Every 30 minutes (UTC); each run reads only the sources that are
      // due (active sources every 3 h, others every 12 h, failing ones
      // back off). Railway skips a run while the previous one is going.
      cronSchedule: "7,37 * * * *",
      restartPolicyType: "NEVER",
    },
  });

  const verification = service("worker-verification", {
    source: source(),
    build,
    start: "jobhunt worker verification",
    env: {
      DATABASE_URL: db.env.DATABASE_URL,
      JOBHUNT_DB_MAX_CONNECTIONS: "4",
    },
    deploy: {
      cronSchedule: "22 * * * *",
      restartPolicyType: "NEVER",
    },
  });

  return project("jobhunt", { resources: [db, api, discovery, verification] });
});
