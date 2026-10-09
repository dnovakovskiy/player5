import { expect, test } from "@playwright/test";
import { app, expectPlayheadAdvances, guard, peakDb, play } from "./support/helpers";

// Every runtime the page can fall back to must start and keep time:
// worklet = wasm in an AudioWorklet; script = wasm in a ScriptProcessor;
// js = the wasm2js core in a ScriptProcessor (CSP without wasm).
for (const mode of ["worklet", "script", "js"] as const) {
  test(`?engine=${mode} starts and the playhead advances`, async ({ page }) => {
    const errors = guard(page);
    await page.goto(`/?engine=${mode}`);
    await play(page);
    await expect(app(page)).toHaveAttribute("data-engine", mode);
    await expect(page.locator("#engine-mode")).toContainText(mode === "worklet" ? "AudioWorklet" : "ScriptProcessor");
    await expectPlayheadAdvances(page, 4);
    await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
    expect(errors).toEqual([]);
  });
}

test("default runtime is the AudioWorklet", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await play(page);
  await expect(app(page)).toHaveAttribute("data-engine", "worklet");
});
