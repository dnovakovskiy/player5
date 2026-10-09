import { expect, test } from "@playwright/test";
import { app, expectClean, expectPlayheadAdvances, guard, peakDb, play, playingStep } from "./support/helpers";

test.afterEach(({ page }) => expectClean(page));

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
    // The playhead follows the heard beat: one step at a time, in order, no
    // flicker back at step boundaries (recorded for 2 s at 124 BPM, about
    // 16 steps).
    const steps = await page.evaluate(
      () =>
        new Promise<number[]>((resolve) => {
          const app = document.getElementById("app")!;
          const seen: number[] = [];
          const obs = new MutationObserver(() => seen.push(Number(app.dataset.playingStep)));
          obs.observe(app, { attributes: true, attributeFilter: ["data-playing-step"] });
          setTimeout(() => {
            obs.disconnect();
            resolve(seen);
          }, 2000);
        }),
    );
    expect(steps.length).toBeGreaterThanOrEqual(12);
    for (let i = 1; i < steps.length; i++) {
      expect((steps[i]! - steps[i - 1]! + 16) % 16, `step ${steps[i - 1]} -> ${steps[i]}`).toBe(1);
    }
    expect(errors).toEqual([]);
  });
}

test("default runtime is the AudioWorklet", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await play(page);
  await expect(app(page)).toHaveAttribute("data-engine", "worklet");
});

// Play, then Stop while the runtime is still starting (the JS core takes the
// longest to load): the engine must end up stopped and silent, not started
// by the first Play resolving late.
for (const mode of ["worklet", "js"] as const) {
  test(`Play then Stop during startup stays stopped (${mode})`, async ({ page }) => {
    guard(page);
    await page.goto(`/?engine=${mode}`);
    const button = page.locator("#play");
    // Both clicks in one task: the second lands while the runtime is still
    // being built, deterministically.
    await button.evaluate((b: HTMLButtonElement) => {
      b.click();
      b.click();
    });
    await expect(page.locator("#status")).toHaveAttribute("data-state", "running", { timeout: 15_000 });
    await expect(button).toHaveAttribute("aria-pressed", "false");
    // If the runtime came up before the Stop, the steps already queued
    // (100 ms lookahead) still sound once; after that the meter falls
    // ~20 dB/s. A running engine would keep it near the kick's peak.
    await expect.poll(() => peakDb(page), { timeout: 6_000 }).toBeLessThan(-45);
    expect(await playingStep(page)).toBe(-1);
    // And a later Play starts exactly once: one bar later the playhead has
    // moved through the steps in order.
    await play(page);
    await expectPlayheadAdvances(page, 4);
  });
}
