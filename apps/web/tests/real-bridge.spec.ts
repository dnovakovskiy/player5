import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createServer } from "node:net";
import { chooseSource, engineTempo, expectClean, expectPlayheadAdvances, guard, play } from "./support/helpers";

// End to end against the real `player5-bridge` binary (apps/bridge), not a
// mock: the bridge serves the built app and the simulated clock, the page
// discovers the bridge on its own origin, connects and locks.

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../../..");
const binary = join(root, "target/debug/player5-bridge");
const dist = join(here, "../dist");

async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.once("error", reject);
    s.listen(0, "127.0.0.1", () => {
      const port = (s.address() as { port: number }).port;
      s.close(() => resolve(port));
    });
  });
}

let bridge: ChildProcess | undefined;
let base = "";

test.beforeAll(async () => {
  test.skip(!existsSync(binary), `build the bridge first: cargo build -p player5-bridge (${binary})`);
  const port = await freePort();
  base = `http://127.0.0.1:${port}`;
  bridge = spawn(
    binary,
    ["--source", "sim", "--sim-bpm", "126", "--bind", "127.0.0.1", "--port", String(port), "--web", dist],
    { stdio: "ignore" },
  );
  // Wait until it answers.
  await expect
    .poll(async () => (await fetch(`${base}/bridge.json`).then((r) => r.ok).catch(() => false)), { timeout: 10_000 })
    .toBe(true);
});

test.afterEach(({ page }) => expectClean(page));

test.afterAll(() => {
  bridge?.kill();
});

test("follows the real bridge's simulated clock from a page the bridge serves", async ({ page }) => {
  const errors = guard(page);
  await page.goto(`${base}/`);
  await play(page);
  await chooseSource(page, "Bridge");
  // Same-origin discovery through /bridge.json.
  await expect(page.locator("#bridge-url")).toHaveValue(`${base.replace("http", "ws")}/ws`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.locator("#clock")).toHaveAttribute("data-bridge", "open");
  await expect(page.locator("#clock")).toHaveAttribute("data-lock", "locked", { timeout: 10_000 });
  await expect
    .poll(async () => Math.abs((await engineTempo(page)) - 126), { timeout: 10_000 })
    .toBeLessThan(0.05);
  await expectPlayheadAdvances(page, 6);
  // The heard beat advances at the bridge's tempo (126 BPM = 2.1 beats per
  // second). data-beat-at is the performance.now() of each data-beat.
  const sample = async () => {
    const clock = page.locator("#clock");
    return {
      beat: Number(await clock.getAttribute("data-beat")),
      at: Number(await clock.getAttribute("data-beat-at")),
    };
  };
  const s0 = await sample();
  await page.waitForTimeout(2_000);
  const s1 = await sample();
  const rate = (s1.beat - s0.beat) / ((s1.at - s0.at) / 1_000);
  expect(Math.abs(rate - 2.1)).toBeLessThan(0.02);
  expect(errors).toEqual([]);
});
