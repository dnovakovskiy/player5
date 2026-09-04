import { defineConfig } from "@playwright/test";
import { existsSync } from "node:fs";

// A pre-installed Chromium (e.g. the remote dev container) may not match
// the revision this Playwright version expects; point at it explicitly.
const preinstalled = "/opt/pw-browsers/chromium";
const executablePath =
  process.env.PW_CHROMIUM ?? (existsSync(preinstalled) ? preinstalled : undefined);

export default defineConfig({
  testDir: "tests",
  timeout: 30_000,
  retries: 0,
  use: {
    browserName: "chromium",
    baseURL: "http://127.0.0.1:4173",
    launchOptions: {
      executablePath,
      // Headless audio: no gesture requirement, no real device needed.
      args: ["--autoplay-policy=no-user-gesture-required"],
    },
  },
  webServer: {
    command: "npm run preview",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
