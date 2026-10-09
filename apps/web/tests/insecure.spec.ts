// The booth case from ADR-0010: apps/bridge serves the app over plain
// http:// on the LAN. That is not a secure context, so there is no
// AudioWorklet, no service worker and no Web MIDI; the app must still play
// (ScriptProcessor runtime) and say why MIDI is missing. A host-resolver
// rule gives the local preview server a non-loopback name.

import { expect, test } from "@playwright/test";
import { existsSync } from "node:fs";
import { app, expectClean, expectPlayheadAdvances, guard, peakDb, play } from "./support/helpers";

test.afterEach(({ page }) => expectClean(page));

const preinstalled = "/opt/pw-browsers/chromium";
test.use({
  launchOptions: {
    executablePath: process.env.PW_CHROMIUM ?? (existsSync(preinstalled) ? preinstalled : undefined),
    args: ["--autoplay-policy=no-user-gesture-required", "--host-resolver-rules=MAP booth.test 127.0.0.1"],
  },
});

test("insecure LAN origin: plays in the ScriptProcessor, explains missing MIDI", async ({ page }) => {
  const errors = guard(page);
  await page.goto("http://booth.test:4173/");
  expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
  expect(await page.evaluate(() => "serviceWorker" in navigator)).toBe(false);
  await expect(page.locator(".seg label", { hasText: /^MIDI$/ })).toBeHidden();
  await expect(page.locator(".seg label", { hasText: /^Bridge$/ })).toBeVisible();
  await expect(page.locator("#clock-unavailable")).toContainText("secure page");
  await play(page);
  await expect(app(page)).toHaveAttribute("data-engine", "script");
  await expectPlayheadAdvances(page, 4);
  await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
  expect(errors).toEqual([]);
});
