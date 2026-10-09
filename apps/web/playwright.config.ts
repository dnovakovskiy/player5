import { defineConfig } from "@playwright/test";
import { existsSync } from "node:fs";

// A pre-installed Chromium (e.g. the remote dev container) may not match
// the revision this Playwright version expects; point at it explicitly.
// CI installs Playwright's own Chromium instead.
const preinstalled = "/opt/pw-browsers/chromium";
const executablePath = process.env.PW_CHROMIUM ?? (existsSync(preinstalled) ? preinstalled : undefined);

// `vite preview` binds 127.0.0.1 explicitly (package.json): on CI runners
// "localhost" resolves to ::1 and the server would be unreachable here.
const BASE_URL = "http://127.0.0.1:4173";

export default defineConfig({
  testDir: "tests",
  timeout: 45_000,
  expect: { timeout: 10_000 },
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : 3,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    browserName: "chromium",
    baseURL: BASE_URL,
    launchOptions: {
      executablePath,
      // Headless audio: no gesture requirement, no real device needed.
      args: ["--autoplay-policy=no-user-gesture-required"],
    },
  },
  webServer: {
    command: "npm run preview",
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
