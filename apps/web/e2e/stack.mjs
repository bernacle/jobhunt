// Starts everything the end-to-end tests (and local development) need,
// with no live job board and no real email:
//
//   1. a fresh Postgres database (jobhunt_e2e);
//   2. the fixture job boards (fixture-server.mjs);
//   3. `narrow migrate`, then the real discovery worker over the fixtures;
//   4. `narrow server` (dev auth, file email, verification against the
//      fixtures);
//   5. the web app (`next start`, which needs `npm run build` first; or
//      `next dev` with --dev).
//
// Playwright runs it as its web server; `node e2e/stack.mjs --dev` runs it
// for local work.
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";

import { ADMIN_DATABASE_URL, JOBHUNT_BIN, MAIL_FILE, PORTS, STATE_DIR, WEB_URL, cloudEnv, webEnv } from "./env.mjs";
import { startFixtureServer } from "./fixture-server.mjs";

const dev = process.argv.includes("--dev");
const children = [];

function stop(code = 0) {
  for (const child of children) child.kill("SIGTERM");
  process.exit(code);
}
process.on("SIGINT", () => stop(0));
process.on("SIGTERM", () => stop(0));

/** Runs a command to completion without blocking this process (which
 * serves the fixture boards the command may be reading). */
function run(label, command, args, env) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("close", (code) => {
      if (code !== 0) {
        console.error(`[stack] ${label} failed:\n${stdout}\n${stderr}`);
        stop(1);
      }
      console.log(`[stack] ${label}: ok`);
      resolve(stdout);
    });
  });
}

function start(label, command, args, env) {
  const child = spawn(command, args, { env, stdio: ["ignore", "inherit", "inherit"] });
  child.on("exit", (code) => {
    if (code !== 0 && code !== null) {
      console.error(`[stack] ${label} exited with ${code}`);
      stop(1);
    }
  });
  children.push(child);
  return child;
}

async function waitFor(url, label) {
  for (let i = 0; i < 240; i++) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  console.error(`[stack] ${label} did not start (${url})`);
  stop(1);
}

rmSync(STATE_DIR, { recursive: true, force: true });
mkdirSync(STATE_DIR, { recursive: true });

await run("fresh database", "psql", [
  ADMIN_DATABASE_URL,
  "-v",
  "ON_ERROR_STOP=1",
  "-q",
  "-c",
  "DROP DATABASE IF EXISTS jobhunt_e2e WITH (FORCE)",
  "-c",
  "CREATE DATABASE jobhunt_e2e",
]);

await startFixtureServer(PORTS.fixtures);
console.log(`[stack] fixture boards on port ${PORTS.fixtures}`);

const env = cloudEnv();
await run("migrations", JOBHUNT_BIN, ["migrate"], env);
const discovered = JSON.parse(await run("discovery worker", JOBHUNT_BIN, ["worker", "discovery"], env));
console.log(`[stack] discovered ${discovered.new} jobs from ${discovered.sources_read} boards`);

start("api", JOBHUNT_BIN, ["server"], env);
await waitFor(`http://127.0.0.1:${PORTS.api}/ready`, "the API");

start("web", "npx", dev ? ["next", "dev", "--port", String(PORTS.web)] : ["next", "start", "--port", String(PORTS.web)], webEnv());
await waitFor(`${WEB_URL}/healthz`, "the web app");
console.log(`[stack] ready: ${WEB_URL} (emails in ${MAIL_FILE})`);
