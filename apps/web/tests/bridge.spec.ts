import { expect, test } from "@playwright/test";
import { chooseSource, engineTempo, expectClean, expectPlayheadAdvances, guard, play } from "./support/helpers";
import type { Page } from "@playwright/test";
import { startMockBridge, type MockBridge } from "./support/mock-bridge";

test.afterEach(({ page }) => expectClean(page));

let bridge: MockBridge;

test.beforeEach(async () => {
  bridge = await startMockBridge({ bpm: 128, precision: "fine" });
});

test.afterEach(async () => {
  await bridge.close();
});

/**
 * In phase with the bridge's bar (anchor beat 1033.25: bar position 1.25 at
 * the anchor). `data-beat` is the engine's beat as heard when it was
 * written (`data-beat-at`); extrapolated to now it must match the server's
 * beat now. This checks the offset estimate, the timeline formula, bar
 * alignment and the follower. (It cannot see a constant error in
 * getOutputTimestamp itself: the same mapping is used in both directions.)
 * Measured: ~1 ms in the worklet, ~7 ms jitter with a ScriptProcessor;
 * 0.1 beat is ~47 ms at 128 BPM.
 */
async function expectInPhase(page: Page, bpm = 128): Promise<void> {
  await expect
    .poll(
      async () => {
        const [beat, age] = await page.evaluate(() => {
          const el = document.getElementById("clock")!;
          return [Number(el.dataset.beat), performance.now() - Number(el.dataset.beatAt)];
        });
        const ours = beat! + (age! / 1000) * (bpm / 60);
        let d = (ours - bridge.beatNow()) % 4;
        if (d > 2) d -= 4;
        if (d < -2) d += 4;
        return Math.abs(d);
      },
      { timeout: 20_000 },
    )
    .toBeLessThan(0.1);
}

// Choosing Bridge connects to the default URL first; with no bridge there
// Chromium logs the refused connection. That is the only console error
// these tests accept.
const REFUSED = [/WebSocket connection to 'ws:\/\/localhost:17505\/ws' failed/];

test("BridgeClock: locks to the bridge timeline and lists devices", async ({ page }) => {
  const errors = guard(page, REFUSED);
  await page.goto("/");
  await play(page);
  await chooseSource(page, "Bridge");
  await page.locator("#bridge-url").fill(bridge.url);
  await page.getByRole("button", { name: "Connect" }).click();

  // Ping burst for the clock offset.
  await expect.poll(() => bridge.received.filter((m) => m.type === "ping").length).toBeGreaterThanOrEqual(8);
  await expect(page.locator("#clock")).toHaveAttribute("data-bridge", "open");
  await expect
    .poll(async () => Math.abs((await engineTempo(page)) - 128), { timeout: 20_000 })
    .toBeLessThan(0.5);
  await expect(page.locator("#clock")).toHaveAttribute("data-lock", "locked");
  await expect(page.locator("#source-status")).toContainText("128.00 BPM");
  await expectInPhase(page);
  await expectPlayheadAdvances(page, 4);

  // Devices and follow targets.
  const deck = page.locator('#bridge-devices li[data-device="2"]');
  await expect(deck).toContainText("Mock Deck");
  await expect(deck).toContainText("master");
  await expect(page.locator('#bridge-devices li[data-device="33"]')).toContainText("mixer");
  const follow = page.getByRole("combobox", { name: "Follow" });
  await expect(follow.locator("option")).toHaveText(["Tempo master", "2 · Mock Deck", "3 · Other Deck"]);
  await follow.selectOption("3");
  await expect.poll(() => bridge.received.find((m) => m.type === "follow")).toEqual({ type: "follow", target: 3 });
  await follow.selectOption("master");
  await expect
    .poll(() => bridge.received.filter((m) => m.type === "follow").at(-1))
    .toEqual({ type: "follow", target: "master" });

  // The URL is remembered.
  await page.reload();
  await chooseSource(page, "Bridge");
  await expect(page.locator("#bridge-url")).toHaveValue(bridge.url);
  expect(errors).toEqual([]);
});

test("BridgeClock: reconnects after the bridge drops", async ({ page }) => {
  // While the bridge is down each retry logs a refused connection.
  guard(page, [...REFUSED, /WebSocket connection to 'ws:\/\/127\.0\.0\.1:\d+\/ws' failed/]);
  await page.goto("/");
  await chooseSource(page, "Bridge");
  await page.locator("#bridge-url").fill(bridge.url);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.locator("#clock")).toHaveAttribute("data-bridge", "open");
  // Follow a specific deck, then lose the bridge.
  await page.getByRole("combobox", { name: "Follow" }).selectOption("3");
  await expect.poll(() => bridge.received.find((m) => m.type === "follow")).toEqual({ type: "follow", target: 3 });
  const url = new URL(bridge.url);
  await bridge.close();
  await expect(page.locator("#clock")).not.toHaveAttribute("data-bridge", "open");
  // Bring a bridge back on the same port; the client retries with backoff.
  bridge = await startMockBridge({ port: Number(url.port) });
  await expect(page.locator("#clock")).toHaveAttribute("data-bridge", "open", { timeout: 15_000 });
  // A restarted bridge follows "master" again: the page re-sends the
  // target it shows.
  await expect.poll(() => bridge.received.find((m) => m.type === "follow")).toEqual({ type: "follow", target: 3 });
  await expect(page.getByRole("combobox", { name: "Follow" })).toHaveValue("3");
});

test("bridge discovery: GET /bridge.json only once Bridge is chosen", async ({ page }) => {
  // A page served by the bridge finds its WebSocket on the same origin.
  // vite preview is not a bridge, so the discovery answer is faked here.
  const errors = guard(page, [/WebSocket connection to 'ws:\/\/127\.0\.0\.1:4173\/ws' failed/]);
  let asked = 0;
  await page.route("**/bridge.json", (route) => {
    asked++;
    return route.fulfill({ contentType: "application/json", body: JSON.stringify({ protocol: 1, ws: "/ws" }) });
  });
  await page.goto("/");
  await page.waitForTimeout(500);
  expect(asked).toBe(0); // static hosts would log a 404 on every load otherwise
  await chooseSource(page, "Bridge");
  await expect(page.locator("#bridge-url")).toHaveValue("ws://127.0.0.1:4173/ws");
  expect(asked).toBe(1);
  await expect(page.locator("#source-status")).toContainText("127.0.0.1:4173/ws");
  // Offline reads as one sentence, then the hint; no bare close code 1006
  // (which only repeats "offline").
  await expect(page.locator("#source-status")).toHaveText(/retrying\. Press Play to start/);
  await expect(page.locator("#source-status")).not.toContainText("1006");
  expect(errors).toEqual([]);
});

test("BridgeClock in the ScriptProcessor fallback: same lock and phase", async ({ page }) => {
  const errors = guard(page, REFUSED);
  await page.goto("/?engine=script");
  await play(page);
  await chooseSource(page, "Bridge");
  await page.locator("#bridge-url").fill(bridge.url);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect
    .poll(async () => Math.abs((await engineTempo(page)) - 128), { timeout: 20_000 })
    .toBeLessThan(0.5);
  await expect(page.locator("#clock")).toHaveAttribute("data-lock", "locked");
  await expectInPhase(page);
  expect(errors).toEqual([]);
});
