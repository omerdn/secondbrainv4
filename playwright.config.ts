import { defineConfig } from "@playwright/test";

/**
 * End-to-end tests of critical web flows (T2.5.7). They run against a local stack, started
 * beforehand: `.claude/launch.json` → `server` (API on :8080) and `client-web` (web on :8081).
 * Uses the installed Google Chrome, so no browser download is needed. Not part of CI.
 */
export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  reporter: "list",
  use: {
    baseURL: process.env.E2E_WEB_URL ?? "http://localhost:8081",
    channel: "chrome",
    locale: "tr-TR",
    trace: "retain-on-failure",
  },
});
