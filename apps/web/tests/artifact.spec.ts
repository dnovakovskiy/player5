// The artifact fragment (npm run build:artifact →
// dist-single/player5-artifact.html) the way a publishing host serves it:
// wrapped in that host's own document skeleton under a strict CSP. These
// tests load the fragment exactly like that, at desktop and phone widths.

import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { base64Url, expectClean, guard, peakDb, play, step, VOICE_IDS } from "./support/helpers";

test.afterEach(({ page }) => expectClean(page));

const FRAGMENT = join(import.meta.dirname, "../dist-single/player5-artifact.html");
const CSP = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'";
const SHOTS = process.env.P5_SHOTS;

/**
 * The host's skeleton: charset and CSP only. Whether a host adds a
 * viewport meta is out of our hands, so the fragment must bring its own.
 */
function wrapped(): string {
  return (
    `<!doctype html><html><head><meta charset="utf-8">` +
    `<meta http-equiv="Content-Security-Policy" content="${CSP}"></head>` +
    `<body>${readFileSync(FRAGMENT, "utf8")}</body></html>`
  );
}

function recordRequests(page: Page): string[] {
  const requests: string[] = [];
  page.on("request", (r) => requests.push(r.url()));
  page.on("websocket", (ws) => requests.push(ws.url()));
  return requests;
}

async function noHorizontalScroll(page: Page, width: number): Promise<void> {
  const m = await page.evaluate(() => ({
    scroll: Math.max(document.documentElement.scrollWidth, document.body.scrollWidth),
    client: document.documentElement.clientWidth,
    inner: window.innerWidth,
  }));
  expect(m.scroll, "horizontal page scroll").toBeLessThanOrEqual(m.client);
  expect(m.inner).toBe(m.client);
  expect(m.client, "layout viewport is the device width").toBe(width);
}

for (const [width, height] of [
  [1280, 900],
  [390, 844],
] as const) {
  test(`artifact fragment in a strict-CSP host page at ${width} px: plays, no scroll, no requests`, async ({
    browser,
  }) => {
    // A phone is emulated for real (isMobile): without a viewport meta the
    // page would lay out at 980 px and be scaled down to unreadable.
    const context = await browser.newContext({
      viewport: { width, height },
      isMobile: width < 500,
      hasTouch: width < 500,
    });
    const page = await context.newPage();
    const errors = guard(page);
    const requests = recordRequests(page);
    await page.setContent(wrapped(), { waitUntil: "load" });
    await expect(page).toHaveTitle("player5");
    await expect(page.locator("#status")).toHaveText(/press Play/);
    await noHorizontalScroll(page, width);
    if (SHOTS) await page.screenshot({ path: join(SHOTS, `artifact-${width}.png`), fullPage: true });
    await play(page);
    await expect(page.locator("#app")).toHaveAttribute("data-engine", "js");
    await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
    await noHorizontalScroll(page, width);
    // Every interactive control fits the screen.
    const overflow = await page.evaluate(() =>
      [...document.querySelectorAll("button, input, select, output")]
        .filter((el) => (el as HTMLElement).offsetParent !== null)
        .filter((el) => {
          const r = el.getBoundingClientRect();
          return r.left < 0 || r.right > document.documentElement.clientWidth;
        })
        .map((el) => el.id || el.textContent),
    );
    expect(overflow).toEqual([]);
    if (SHOTS) await page.screenshot({ path: join(SHOTS, `artifact-${width}-playing.png`), fullPage: true });
    expect(requests).toEqual([]);
    expect(errors).toEqual([]);
    await context.close();
  });
}

test("artifact fragment: pattern code share and import round-trip", async ({ page }) => {
  const errors = guard(page);
  const requests = recordRequests(page);
  await page.setContent(wrapped(), { waitUntil: "load" });
  await page.getByRole("button", { name: "Clear" }).click();
  await step(page, "clap", 5).click();
  await step(page, "open_hat", 15).click();
  await page.getByRole("button", { name: "Share" }).click();
  await expect(page.locator("#share-msg")).toContainText("Pattern code");
  const code = await page.locator("#share-code").inputValue();
  await page.getByRole("button", { name: "Clear" }).click();
  await expect(step(page, "clap", 5)).toHaveAttribute("data-state", "off");
  await page.locator("#import").fill(code);
  await page.locator("#import").press("Enter");
  await expect(page.locator("#share-msg")).toHaveText("Pattern loaded.");
  await expect(step(page, "clap", 5)).toHaveAttribute("data-state", "on");
  await expect(step(page, "open_hat", 15)).toHaveAttribute("data-state", "on");
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});

test("artifact fragment: every voice on its own reaches the meter", async ({ page }) => {
  test.setTimeout(90_000);
  const errors = guard(page);
  await page.setContent(wrapped(), { waitUntil: "load" });
  const levels: Record<string, number> = {};
  for (const voice of VOICE_IDS) {
    const code = base64Url(JSON.stringify({ bpm: 140, voices: { [voice]: { steps: "X-x-X-x-X-x-X-x-" } } }));
    await page.locator("#import").fill(code);
    await page.locator("#import").press("Enter");
    await expect(step(page, voice, 1)).toHaveAttribute("data-state", "accent");
    // Silence first: the meter must have decayed from the previous voice.
    await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeLessThan(-40);
    await play(page);
    let max = -Infinity;
    const until = Date.now() + 1_500;
    while (Date.now() < until) {
      max = Math.max(max, await peakDb(page));
      await page.waitForTimeout(30);
    }
    levels[voice] = max;
    await page.getByRole("button", { name: "Stop", exact: true }).click();
  }
  for (const voice of VOICE_IDS) expect(levels[voice], `${voice} level (${JSON.stringify(levels)})`).toBeGreaterThan(-30);
  expect(errors).toEqual([]);
});
