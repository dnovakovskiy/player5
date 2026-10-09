import { expect, test } from "@playwright/test";
import { chooseSource, engineTempo, expectPlayheadAdvances, guard, play } from "./support/helpers";
import { startMockBridge, type MockBridge } from "./support/mock-bridge";

let bridge: MockBridge;

test.beforeEach(async () => {
  bridge = await startMockBridge({ bpm: 128, precision: "fine" });
});

test.afterEach(async () => {
  await bridge.close();
});

test("BridgeClock: locks to the bridge timeline and lists devices", async ({ page }) => {
  const errors = guard(page);
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
  // In phase with the bridge's bar (anchor beat 1033.25 → bar position 1.25
  // at the anchor): the time mapping (offset estimate, getOutputTimestamp,
  // engine frame offset) puts our beat where the server's is. Tolerance
  // covers output latency and the 20 Hz status age (~0.1 beat each).
  await expect
    .poll(
      async () => {
        const ours = Number(await page.locator("#clock").getAttribute("data-beat"));
        let d = (ours - bridge.beatNow()) % 4;
        if (d > 2) d -= 4;
        if (d < -2) d += 4;
        return Math.abs(d);
      },
      { timeout: 10_000 },
    )
    .toBeLessThan(0.35);
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
  guard(page);
  await page.goto("/");
  await chooseSource(page, "Bridge");
  await page.locator("#bridge-url").fill(bridge.url);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.locator("#clock")).toHaveAttribute("data-bridge", "open");
  const url = new URL(bridge.url);
  await bridge.close();
  await expect(page.locator("#clock")).not.toHaveAttribute("data-bridge", "open");
  // Bring a bridge back on the same port; the client retries with backoff.
  bridge = await startMockBridge({ port: Number(url.port) });
  await expect(page.locator("#clock")).toHaveAttribute("data-bridge", "open", { timeout: 15_000 });
});
