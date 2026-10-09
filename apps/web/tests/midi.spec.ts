import { expect, test } from "@playwright/test";
import { chooseSource, engineTempo, expectPlayheadAdvances, guard, play } from "./support/helpers";

// A fake Web MIDI input that sends Start, then Timing Clock at 125 BPM
// (24 ppqn = one pulse every 20 ms). Each message carries its ideal
// timestamp on the performance clock, like a real MIDIMessageEvent.
test("MIDI clock at 125 BPM: engine follows and locks", async ({ page }) => {
  const errors = guard(page);
  await page.addInitScript(() => {
    const BPM = 125;
    const period = 60_000 / BPM / 24;
    type Handler = ((e: { data: Uint8Array; timeStamp: number }) => void) | null;
    const input: { id: string; name: string; manufacturer: string; state: string; type: string; onmidimessage: Handler } = {
      id: "fake-clock",
      name: "Fake Clock",
      manufacturer: "Test",
      state: "connected",
      type: "input",
      onmidimessage: null,
    };
    let t0: number | null = null;
    let k = 0;
    setInterval(() => {
      const send = input.onmidimessage;
      if (!send) return;
      const now = performance.now();
      if (t0 === null) {
        t0 = now;
        send({ data: new Uint8Array([0xfa]), timeStamp: now });
      }
      while (t0 + k * period <= now) {
        send({ data: new Uint8Array([0xf8]), timeStamp: t0 + k * period });
        k++;
      }
    }, 5);
    Object.defineProperty(navigator, "requestMIDIAccess", {
      configurable: true,
      value: async () => ({
        inputs: new Map([[input.id, input]]),
        outputs: new Map(),
        sysexEnabled: false,
        onstatechange: null,
      }),
    });
  });
  await page.goto("/");
  await play(page);
  await chooseSource(page, "MIDI");
  await expect(page.locator("#midi-input")).toHaveValue("fake-clock");
  await expect(page.locator("#source-status")).toContainText("Fake Clock");
  await expect
    .poll(async () => Math.abs((await engineTempo(page)) - 125), { timeout: 20_000 })
    .toBeLessThan(0.5);
  await expect(page.locator("#clock")).toHaveAttribute("data-lock", "locked");
  await expect(page.locator("#clock-lock")).toHaveText("locked");
  // While following, the tempo field shows the source and is read-only.
  await expect(page.locator("#bpm")).toBeDisabled();
  await expect.poll(async () => Number(await page.locator("#bpm").inputValue())).toBeCloseTo(125, 0);
  await expectPlayheadAdvances(page, 4);
  // Back to internal: the followed tempo is kept.
  await chooseSource(page, "Internal");
  await expect(page.locator("#clock")).toHaveAttribute("data-lock", "internal");
  await expect(page.locator("#bpm")).toBeEnabled();
  expect(Math.abs(Number(await page.locator("#bpm").inputValue()) - 125)).toBeLessThan(0.5);
  expect(errors).toEqual([]);
});
