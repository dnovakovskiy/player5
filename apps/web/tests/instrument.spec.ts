import { expect, test } from "@playwright/test";
import {
  app,
  base64Url,
  expectClean,
  expectPhoneLayout,
  expectPlayheadAdvances,
  guard,
  peakDb,
  play,
  reopen,
  settledUrl,
  step,
  VOICE_IDS,
} from "./support/helpers";

test.afterEach(({ page }) => expectClean(page));

test("plays: playhead advances, meter shows a level, Stop clears", async ({ page }) => {
  const errors = guard(page);
  await page.goto("/");
  await expect(app(page)).toHaveAttribute("data-playing-step", "-1");
  await play(page);
  await expectPlayheadAdvances(page, 4);
  // The default groove has kicks: the decaying peak shows real level.
  await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
  await expect(page.locator("#meter")).toHaveAttribute("aria-valuetext", /dBFS/);
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(app(page)).toHaveAttribute("data-playing-step", "-1");
  expect(errors).toEqual([]);
});

test("Space toggles play from anywhere but text fields", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await page.locator("body").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("Space");
  await expect(page.locator("#status")).toHaveAttribute("data-state", "running", { timeout: 15_000 });
  await expect(page.locator("#play")).toHaveAttribute("aria-pressed", "true");
  // A focused step button: Space still means play/stop, not "toggle step".
  await step(page, "kick", 2).focus();
  await page.keyboard.press("Space");
  await expect(page.locator("#play")).toHaveAttribute("aria-pressed", "false");
  await expect(step(page, "kick", 2)).toHaveAttribute("data-state", "off");
  // Enter toggles the focused step; arrows move.
  await page.keyboard.press("Enter");
  await expect(step(page, "kick", 2)).toHaveAttribute("data-state", "on");
  await page.keyboard.press("ArrowDown");
  await expect(step(page, "snare", 2)).toBeFocused();
});

test("every voice toggles and round-trips through the URL", async ({ page }) => {
  const errors = guard(page);
  await page.goto("/");
  await play(page); // the core validates every pattern we send it
  await page.getByRole("button", { name: "Clear" }).click();
  for (const [i, voice] of VOICE_IDS.entries()) {
    const n = i + 1;
    const s = step(page, voice, n);
    await expect(s).toHaveAttribute("data-state", "off");
    await s.click();
    await expect(s).toHaveAttribute("data-state", "on");
    await step(page, voice, 16 - i).click();
    await step(page, voice, 16 - i).click();
    await expect(step(page, voice, 16 - i)).toHaveAttribute("data-state", "accent");
  }
  await expect(page.locator("#status")).toHaveAttribute("data-state", "running");
  const url = await settledUrl(page);
  expect(url).toContain("#p=");
  await page.goto("about:blank");
  await page.goto(url);
  for (const [i, voice] of VOICE_IDS.entries()) {
    await expect(step(page, voice, i + 1)).toHaveAttribute("data-state", "on");
    await expect(step(page, voice, 16 - i)).toHaveAttribute("data-state", "accent");
  }
  // Untouched steps stay off.
  await expect(step(page, "cowbell", 1)).toHaveAttribute("data-state", "off");
  expect(errors).toEqual([]);
});

test("flam edit mode toggles flams with distinct states", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Clear" }).click();
  const a = step(page, "snare", 5);
  const b = step(page, "snare", 13);
  await a.click(); // hit
  await b.click();
  await b.click(); // accent
  const flamMode = page.getByRole("button", { name: "Flam edit" });
  await flamMode.click();
  await expect(flamMode).toHaveAttribute("aria-pressed", "true");
  await a.click();
  await b.click();
  await expect(a).toHaveAttribute("data-state", "flam");
  await expect(b).toHaveAttribute("data-state", "accent-flam");
  // An empty step becomes a flammed hit; toggling again leaves the hit.
  const c = step(page, "snare", 7);
  await c.click();
  await expect(c).toHaveAttribute("data-state", "flam");
  await c.click();
  await expect(c).toHaveAttribute("data-state", "on");
  await flamMode.click();
  // Normal taps keep the flam through hit → accent → off.
  await a.click();
  await expect(a).toHaveAttribute("data-state", "accent-flam");
  await a.click();
  await expect(a).toHaveAttribute("data-state", "off");
  // Persisted as f/F in the pattern.
  await reopen(page);
  await expect(b).toHaveAttribute("data-state", "accent-flam");
});

test("mute silences a voice and survives a reload", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await page.getByRole("combobox", { name: "Load a preset pattern" }).selectOption("bare-kick");
  await play(page);
  await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
  const mute = page.getByRole("button", { name: "Mute Bass drum" }).first();
  await mute.click();
  await expect(page.locator('.row[data-voice="kick"]')).toHaveClass(/muted/);
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  // The meter falls ~20 dB/s once the kick stops.
  await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeLessThan(-45);
  await reopen(page);
  await expect(page.locator('.row[data-voice="kick"]')).toHaveClass(/muted/);
});

test("presets load original patterns and undo restores", async ({ page }) => {
  guard(page);
  await page.goto("/");
  const select = page.getByRole("combobox", { name: "Load a preset pattern" });
  const options = await select.locator("option").allTextContents();
  expect(options.length).toBeGreaterThanOrEqual(5); // placeholder + 4 or more
  for (const name of ["Basement Four", "Night Shift", "Circuit Break", "Late Swing"]) {
    expect(options).toContain(name);
  }
  await select.selectOption({ label: "Night Shift" });
  await expect(page.locator("#bpm")).toHaveValue("132");
  await expect(step(page, "closed_hat", 3)).toHaveAttribute("data-state", "accent");
  await expect(step(page, "low_tom", 3)).toHaveAttribute("data-state", "on");
  await select.selectOption({ label: "Late Swing" });
  await expect(page.locator("#bpm")).toHaveValue("112");
  await expect(step(page, "snare", 5)).toHaveAttribute("data-state", "accent-flam");
  await expect(page.locator("#shuffle-out")).toHaveText("60%");
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.locator("#bpm")).toHaveValue("132");
  await page.keyboard.press("Control+z");
  await expect(page.locator("#bpm")).toHaveValue("124");
  await page.keyboard.press("Control+Shift+z");
  await expect(page.locator("#bpm")).toHaveValue("132");
});

test("an old kick-only link still loads", async ({ page }) => {
  guard(page);
  // Exactly what the first web shell wrote into the URL.
  const old = {
    bpm: 128,
    shuffle: 0.25,
    accent: 1,
    voices: { kick: { steps: "X---x---X---x-x-", tune: 0.3, decay: 0.7, level: 0.9 } },
    render: { output_gain: 1, limiter: true },
  };
  await page.goto(`/#p=${base64Url(JSON.stringify(old))}`);
  await expect(page.locator("#bpm")).toHaveValue("128");
  await expect(step(page, "kick", 1)).toHaveAttribute("data-state", "accent");
  await expect(step(page, "kick", 5)).toHaveAttribute("data-state", "on");
  await expect(step(page, "kick", 15)).toHaveAttribute("data-state", "on");
  await expect(step(page, "kick", 2)).toHaveAttribute("data-state", "off");
  await expect(step(page, "snare", 1)).toHaveAttribute("data-state", "off");
  await expect(page.locator("#limiter")).toBeChecked();
  await expect(page.locator("#vc-tune-out")).toHaveText("30%");
  await expect(page.locator("#shuffle-out")).toHaveText("25%");
  await play(page);
  await expectPlayheadAdvances(page, 3);
});

test("voice panel shows each voice's controls", async ({ page }) => {
  guard(page);
  await page.goto("/");
  const expected: Record<string, string[]> = {
    BD: ["tune", "decay", "level"],
    SD: ["tune", "tone", "snappy", "decay", "level"],
    LT: ["tune", "decay", "level"],
    RS: ["tune", "tone", "decay", "level"],
    CP: ["tone", "decay", "level"],
    CH: ["tone", "decay", "level"],
    OH: ["tone", "decay", "level"],
    CB: ["tune", "tone", "decay", "level"],
  };
  for (const [short, controls] of Object.entries(expected)) {
    await page.locator(".voice-btn", { hasText: new RegExp(`^${short}$`) }).click();
    const got = await page.locator("#voice-controls input[data-control]").evaluateAll((els) =>
      els.map((e) => (e as HTMLInputElement).dataset.control),
    );
    expect(got, short).toEqual(controls);
  }
  // A control change lands in the pattern.
  await page.locator(".voice-btn", { hasText: /^SD$/ }).click();
  await page.locator("#vc-snappy").fill("0.8");
  await expect(page.locator("#vc-snappy-out")).toHaveText("80%");
  await reopen(page);
  await page.locator(".voice-btn", { hasText: /^SD$/ }).click();
  await expect(page.locator("#vc-snappy")).toHaveValue("0.8");
});

test("master: gain in dB and limiter round-trip", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await page.locator("#gain").fill("-6");
  await expect(page.locator("#gain-out")).toHaveText("−6.0 dB");
  await page.locator("#limiter").check();
  await reopen(page);
  await expect(page.locator("#gain-out")).toHaveText("−6.0 dB");
  await expect(page.locator("#limiter")).toBeChecked();
});

test("share and import without dialogs", async ({ page }) => {
  const errors = guard(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Share" }).click();
  await expect(page.locator("#share-msg")).toContainText(/Link (copied|selected)/);
  const link = await page.locator("#share-link").inputValue();
  expect(link).toContain("#p=");
  const code = await page.locator("#share-code").inputValue();
  expect(link.endsWith(code)).toBe(true);
  await page.getByRole("button", { name: "Clear" }).click();
  await expect(step(page, "kick", 1)).toHaveAttribute("data-state", "off");
  await page.locator("#import").fill(link);
  await page.locator("#import").press("Enter");
  await expect(page.locator("#share-msg")).toHaveText("Pattern loaded.");
  await expect(step(page, "kick", 1)).toHaveAttribute("data-state", "accent");
  await page.locator("#import").fill("not a code!");
  await page.getByRole("button", { name: "Load" }).click();
  await expect(page.locator("#share-msg")).toContainText("not a player5 pattern");
  expect(errors).toEqual([]);
});

test("phone width: no horizontal scroll, 16 px gutter", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await expectPhoneLayout(page);
  await play(page);
  await expectPhoneLayout(page);
});

test("tap tempo sets the BPM and reaches the engine", async ({ page }) => {
  guard(page);
  await page.goto("/");
  await play(page);
  const tap = page.locator("#tap");
  for (let i = 0; i < 5; i++) {
    await tap.dispatchEvent("pointerdown");
    await page.waitForTimeout(500);
  }
  // 500 ms apart ≈ 120 BPM (timer and event jitter allowed).
  await expect.poll(async () => Number(await page.locator("#bpm").inputValue())).toBeGreaterThan(110);
  expect(Number(await page.locator("#bpm").inputValue())).toBeLessThan(130);
  await expect
    .poll(async () => Number(await page.locator("#clock").getAttribute("data-tempo")))
    .toBeCloseTo(Number(await page.locator("#bpm").inputValue()), 1);
});

test("garbage in the hash or the import field never breaks the page", async ({ page }) => {
  const errors = guard(page);
  const hostile = {
    bpm: "fast",
    shuffle: 7,
    accent: -3,
    flam: null,
    voices: {
      kick: { steps: 42, tune: "x", level: 1e999 },
      snare: { steps: "<img src=x onerror=alert(1)> XXXX", mute: "yes" },
      cymbal: { steps: "xxxx" },
    },
    render: { output_gain: -5, limiter: "yes", bars: 99 },
  };
  const hashes = [
    "#p=!!!",
    "#p=A",
    "#p=AAAA",
    `#p=${base64Url("[]")}`,
    `#p=${base64Url("null")}`,
    `#p=${base64Url('"a string"')}`,
    `#p=${base64Url('{"__proto__":{"polluted":1},"voices":{"__proto__":{"steps":"xxxx"}}}')}`,
    `#p=${base64Url(JSON.stringify(hostile))}`,
    `#p=${"A".repeat(100_000)}`,
  ];
  for (const hash of hashes) {
    await page.goto("about:blank");
    await page.goto(`/${hash}`);
    await expect(page.locator("#bpm"), hash.slice(0, 40)).toHaveValue(/^\d+(\.\d+)?$/);
    // The page settles on a valid pattern and writes it back to the URL.
    const code = await page.locator("#share-code").inputValue();
    expect(code).toMatch(/^[A-Za-z0-9_-]+$/);
    await expect.poll(() => page.url()).toContain(`#p=${code}`);
    expect(await page.evaluate(() => "polluted" in ({} as object))).toBe(false);
  }
  // The hostile object, sanitised: defaults and clamps, no stray markup.
  await page.goto("about:blank");
  await page.goto(`/#p=${base64Url(JSON.stringify(hostile))}`);
  await expect(page.locator("#bpm")).toHaveValue("120");
  await expect(page.locator("#shuffle-out")).toHaveText("100%");
  await expect(page.locator("#accent-out")).toHaveText("0%");
  await expect(page.locator("#gain-out")).toHaveText("−∞ dB");
  await expect(page.locator("#limiter")).not.toBeChecked();
  await expect(page.locator(".row.muted")).toHaveCount(0);
  await expect(page.locator("#app img")).toHaveCount(0);
  const states = await page
    .locator('.step[data-voice="snare"]')
    .evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.state));
  expect(states).toHaveLength(16);
  for (const s of states) expect(["off", "on", "accent"]).toContain(s);
  // ...and the core accepts it.
  await play(page);
  await expectPlayheadAdvances(page, 3);
  // The import field rejects junk without touching the pattern.
  const before = await page.locator("#share-code").inputValue();
  for (const junk of ["   ", "#p=", "p=%%%", "https://example.com/#p=@@@", "x".repeat(5000)]) {
    await page.locator("#import").fill(junk);
    await page.locator("#import").press("Enter");
    await expect(page.locator("#share-msg")).toContainText("not a player5 pattern");
    await expect(page.locator("#share-code")).toHaveValue(before);
  }
  expect(errors).toEqual([]);
});

test("the skip link focuses the grid and keeps the pattern in the URL", async ({ page }) => {
  guard(page);
  await page.goto("/");
  const url = await settledUrl(page);
  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: "Skip to the pattern" });
  await expect(skip).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(step(page, "kick", 1)).toBeFocused();
  expect(page.url()).toBe(url);
});
