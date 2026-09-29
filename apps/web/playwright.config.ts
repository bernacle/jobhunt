import { defineConfig, devices } from "@playwright/test";

import { WEB_URL } from "./e2e/env.mjs";

/**
 * The product loop in a real browser, against the real stack (Postgres,
 * `narrow server`, the discovery, verification and notification workers)
 * with local fixture job boards and a file instead of an email provider.
 * `e2e/stack.mjs` starts everything; the web app must be built first
 * (`npm run build`).
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : [["list"]],
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: WEB_URL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"] }, testIgnore: /mobile\.spec\.ts/ },
    { name: "mobile", use: { ...devices["Pixel 7"] }, testMatch: /mobile\.spec\.ts/ },
  ],
  webServer: {
    command: "node e2e/stack.mjs",
    url: `${WEB_URL}/healthz`,
    reuseExistingServer: !process.env.CI,
    timeout: 300_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
