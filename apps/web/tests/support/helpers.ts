import { expect, type Page } from "@playwright/test";

export const VOICE_IDS = [
  "kick",
  "snare",
  "low_tom",
  "mid_tom",
  "high_tom",
  "rim",
  "clap",
  "closed_hat",
  "open_hat",
  "cowbell",
] as const;

/** Step button of a voice (steps are 1-based, as labelled). */
export const step = (page: Page, voice: string, n: number) =>
  page.locator(`.step[data-voice="${voice}"][data-step="${n}"]`);

export const app = (page: Page) => page.locator("#app");

const guarded = new WeakMap<Page, string[]>();

/** For `test.afterEach`: a page passed to `guard` logged nothing bad. */
export function expectClean(page: Page): void {
  const errors = guarded.get(page);
  if (errors) expect(errors, "page errors, console errors or dialogs").toEqual([]);
}

/**
 * Collects page errors, console errors and any alert/confirm/prompt dialog
 * (tests assert the list stays empty). `allowConsole` matches console
 * errors a test expects, e.g. the CSP refusal that selects the JS core.
 */
export function guard(page: Page, allowConsole: RegExp[] = []): string[] {
  const errors: string[] = [];
  guarded.set(page, errors);
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    const text = m.text();
    if (!allowConsole.some((re) => re.test(text))) errors.push(`console error: ${text}`);
  });
  page.on("dialog", (d) => {
    errors.push(`unexpected ${d.type()} dialog: ${d.message()}`);
    void d.dismiss();
  });
  return errors;
}

export async function play(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(page.locator("#status")).toHaveAttribute("data-state", "running", { timeout: 15_000 });
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toBeVisible();
}

export async function playingStep(page: Page): Promise<number> {
  return Number(await app(page).getAttribute("data-playing-step"));
}

/** Waits until the playhead has visibly moved through at least `count` distinct steps. */
export async function expectPlayheadAdvances(page: Page, count = 3): Promise<void> {
  const seen = new Set<number>();
  await expect
    .poll(
      async () => {
        const s = await playingStep(page);
        if (s >= 0) seen.add(s);
        return seen.size;
      },
      { timeout: 10_000, intervals: [40] },
    )
    .toBeGreaterThanOrEqual(count);
}

export async function peakDb(page: Page): Promise<number> {
  const v = await page.locator("#meter").getAttribute("data-peak-db");
  return v === null || v === "-inf" ? -Infinity : Number(v);
}

export async function engineTempo(page: Page): Promise<number> {
  const v = await page.locator("#clock").getAttribute("data-tempo");
  return v ? Number(v) : NaN;
}

export function base64Url(s: string): string {
  return Buffer.from(s, "utf8").toString("base64").replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Clicks a clock-source segment by its label. */
export async function chooseSource(page: Page, label: "Internal" | "Tap" | "Bridge" | "MIDI"): Promise<void> {
  await page.locator(".seg label", { hasText: new RegExp(`^${label}$`) }).click();
  await expect(page.locator(`input[name="clock-source"][value="${label.toLowerCase()}"]`)).toBeChecked();
}

/** The page URL once its #p= hash has caught up with the current pattern. */
export async function settledUrl(page: Page): Promise<string> {
  const code = await page.locator("#share-code").inputValue();
  await expect.poll(() => page.url()).toContain(`#p=${code}`);
  return page.url();
}

/** Opens the current pattern's link in a fresh document (a real reload, not a fragment navigation). */
export async function reopen(page: Page): Promise<void> {
  const url = await settledUrl(page);
  await page.goto("about:blank");
  await page.goto(url);
}

/** No horizontal page scroll, and the content keeps a 16 px side gutter. */
export async function expectPhoneLayout(page: Page): Promise<void> {
  await page.setViewportSize({ width: 390, height: 844 });
  const m = await page.evaluate(() => {
    const r = document.getElementById("app")!.getBoundingClientRect();
    return {
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
      bodyScroll: document.body.scrollWidth,
      left: r.left,
      right: window.innerWidth - r.right,
    };
  });
  expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
  expect(m.bodyScroll).toBeLessThanOrEqual(m.clientWidth);
  expect(m.left).toBeGreaterThanOrEqual(16);
  expect(m.right).toBeGreaterThanOrEqual(16);
}
