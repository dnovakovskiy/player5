// The single-file build (npm run build:single → dist-single/player5.html):
// one self-contained page for sandboxed viewers. No network, no service
// worker, JS core fallback when the CSP forbids WebAssembly.

import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { expectClean, expectPhoneLayout, expectPlayheadAdvances, guard, peakDb, play, step } from "./support/helpers";

test.afterEach(({ page }) => expectClean(page));

const FILE = join(import.meta.dirname, "../dist-single/player5.html");
const CSP = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'";

const html = () => readFileSync(FILE, "utf8");

/** Loads the file under a strict CSP the way an embedding viewer would. */
async function loadWithCsp(page: Page): Promise<void> {
  const source = html().replace("<head>", `<head>\n<meta http-equiv="Content-Security-Policy" content="${CSP}">`);
  await page.setContent(source, { waitUntil: "load" });
}

function recordRequests(page: Page, ignore: string[] = []): string[] {
  const requests: string[] = [];
  page.on("request", (r) => {
    if (!ignore.includes(r.url())) requests.push(r.url());
  });
  page.on("websocket", (ws) => requests.push(ws.url()));
  return requests;
}

test("artifact: one file that cannot reach the network", () => {
  const text = html();
  for (const bad of ["http:", "https:", "fetch(", "importScripts", "XMLHttpRequest", "sendBeacon"]) {
    expect(text.includes(bad), `contains ${bad}`).toBe(false);
  }
  expect(text).not.toMatch(/<script[^>]+src=/);
  expect(text).not.toMatch(/<link[^>]+href=/);
  expect(text).not.toContain("serviceWorker");
  expect(text).not.toContain("manifest");
  const title = text.indexOf("<title>player5</title>");
  expect(title).toBeGreaterThan(0);
  expect(title).toBeLessThan(8192);
  expect(text).toMatch(/:root\{[^}]*color-scheme:dark/);
});

test("file:// plays with zero network requests", async ({ page }) => {
  const errors = guard(page);
  const url = pathToFileURL(FILE).href;
  const requests = recordRequests(page, [url]);
  await page.goto(url);
  await expect(page.locator("#app")).toHaveAttribute("data-build", "single");
  await play(page);
  await expectPlayheadAdvances(page, 4);
  await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});

test("strict CSP (no wasm, no blob workers): the JS core plays, zero requests", async ({ page }) => {
  const errors = guard(page);
  const requests = recordRequests(page);
  await loadWithCsp(page);
  await expect(page.locator("#status")).toHaveText(/press Play/);
  // Audio starts only from the Play button.
  await expect(page.locator("#app")).not.toHaveAttribute("data-engine", /./);
  await play(page);
  await expect(page.locator("#app")).toHaveAttribute("data-engine", "js");
  await expectPlayheadAdvances(page, 4);
  await expect.poll(() => peakDb(page), { timeout: 10_000 }).toBeGreaterThan(-30);
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.locator("#app")).toHaveAttribute("data-playing-step", "-1");
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});

test("paints its own dark theme; phone width keeps a 16 px gutter", async ({ page }) => {
  guard(page);
  await loadWithCsp(page);
  const colors = await page.evaluate(() => ({
    html: getComputedStyle(document.documentElement).backgroundColor,
    body: getComputedStyle(document.body).backgroundColor,
    text: getComputedStyle(document.body).color,
    scheme: getComputedStyle(document.documentElement).colorScheme,
  }));
  expect(colors.html).toBe("rgb(15, 17, 21)");
  expect(colors.body).toBe("rgb(15, 17, 21)");
  expect(colors.text).not.toBe("rgb(0, 0, 0)");
  expect(colors.scheme).toBe("dark");
  await expectPhoneLayout(page);
});

test("Bridge and MIDI are hidden with a hint", async ({ page }) => {
  guard(page);
  await loadWithCsp(page);
  await expect(page.locator(".seg label", { hasText: /^Bridge$/ })).toBeHidden();
  await expect(page.locator(".seg label", { hasText: /^MIDI$/ })).toBeHidden();
  await expect(page.locator(".seg label", { hasText: /^Tap$/ })).toBeVisible();
  await expect(page.locator("#clock-unavailable")).toBeVisible();
  await expect(page.locator("#clock-unavailable")).toContainText("Bridge and MIDI");
});

test("pattern code: Share exports it, Import (paste or link) brings it back", async ({ page }) => {
  const errors = guard(page);
  const requests = recordRequests(page);
  await loadWithCsp(page);
  await expect(page.locator("#share-link")).toBeHidden();
  // Make a recognisable pattern.
  await page.getByRole("button", { name: "Clear" }).click();
  await step(page, "rim", 3).click();
  await step(page, "cowbell", 11).click();
  await step(page, "cowbell", 11).click();
  await page.locator("#bpm").fill("133");
  await page.locator("#bpm").press("Enter");

  await page.getByRole("button", { name: "Share" }).click();
  await expect(page.locator("#share-msg")).toContainText(/Pattern code (copied|selected)/);
  const code = await page.locator("#share-code").inputValue();
  expect(code).toMatch(/^[A-Za-z0-9_-]{20,}$/);

  await page.getByRole("button", { name: "Clear" }).click();
  await page.locator("#bpm").fill("90");
  await page.locator("#bpm").press("Enter");
  await expect(step(page, "rim", 3)).toHaveAttribute("data-state", "off");

  // Paste event (clipboard *reads* never work in the viewer; paste does).
  await page.locator("#import").evaluate((el, text) => {
    const data = new DataTransfer();
    data.setData("text/plain", text);
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, code);
  await expect(page.locator("#share-msg")).toHaveText("Pattern loaded.");
  await expect(step(page, "rim", 3)).toHaveAttribute("data-state", "on");
  await expect(step(page, "cowbell", 11)).toHaveAttribute("data-state", "accent");
  await expect(page.locator("#bpm")).toHaveValue("133");
  await expect(page.locator("#share-code")).toHaveValue(code);

  // A full shared link works as well.
  await page.getByRole("button", { name: "Clear" }).click();
  await page.locator("#import").fill(`player5.example/#p=${code}`);
  await page.locator("#import").press("Enter");
  await expect(step(page, "rim", 3)).toHaveAttribute("data-state", "on");
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});

test("inside a sandboxed iframe (opaque origin, strict CSP): plays, edits, shares", async ({ page }) => {
  // The viewer case: no allow-same-origin, so localStorage and the
  // clipboard throw; the page must not care.
  const errors = guard(page);
  const requests = recordRequests(page);
  await page.setContent("<!doctype html><title>viewer</title><body style='margin:0'></body>");
  await page.evaluate(
    ({ source, csp }) => {
      const frame = document.createElement("iframe");
      frame.setAttribute("sandbox", "allow-scripts");
      frame.setAttribute("allow", "autoplay");
      frame.style.cssText = "width:390px;height:800px;border:0";
      frame.srcdoc = source.replace("<head>", `<head><meta http-equiv="Content-Security-Policy" content="${csp}">`);
      document.body.append(frame);
    },
    { source: html(), csp: CSP },
  );
  const frame = page.frameLocator("iframe");
  await expect(frame.locator("#status")).toHaveText(/press Play/);
  await frame.locator(".step[data-voice='cowbell'][data-step='3']").click();
  await expect(frame.locator(".step[data-voice='cowbell'][data-step='3']")).toHaveAttribute("data-state", "on");
  await frame.getByRole("button", { name: "Play", exact: true }).click();
  await expect(frame.locator("#status")).toHaveAttribute("data-state", "running", { timeout: 15_000 });
  await expect(frame.locator("#app")).toHaveAttribute("data-engine", "js");
  const seen = new Set<string>();
  await expect
    .poll(
      async () => {
        seen.add((await frame.locator("#app").getAttribute("data-playing-step")) ?? "-1");
        seen.delete("-1");
        return seen.size;
      },
      { timeout: 10_000, intervals: [40] },
    )
    .toBeGreaterThanOrEqual(4);
  await frame.getByRole("button", { name: "Share" }).click();
  await expect(frame.locator("#share-msg")).toContainText("Pattern code");
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});
