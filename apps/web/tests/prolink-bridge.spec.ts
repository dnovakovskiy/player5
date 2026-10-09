import { expect, test, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chooseSource, engineTempo, expectClean, guard, play } from "./support/helpers";

// The whole chain with Pro DJ Link packets: a fake booth (two players on
// loopback, apps/bridge/examples/fake_booth.rs) → the real bridge with
// `--source prolink` → the app the bridge serves. Checks lock, tempo, bar
// alignment against the players' own downbeats, switching the follow
// target, and reconnecting (and re-following) after the bridge restarts.

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../../..");
const bridgeBin = join(root, "target/debug/player5-bridge");
const boothBin = join(root, "target/debug/examples/fake_booth");
const dist = join(here, "../dist");

interface Downbeat {
  unixMs: number;
  periodMs: number;
}

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

let booth: ChildProcess | undefined;
let bridge: ChildProcess | undefined;
let port = 0;
let portBase = 0;
const downbeats = new Map<number, Downbeat>();

function startBridge(): ChildProcess {
  return spawn(
    bridgeBin,
    [
      "--source", "prolink", "--passive", "--prolink-port-base", String(portBase),
      "--bind", "127.0.0.1", "--port", String(port), "--web", dist,
    ],
    { stdio: "ignore" },
  );
}

async function bridgeUp(): Promise<void> {
  await expect
    .poll(async () => fetch(`http://127.0.0.1:${port}/bridge.json`).then((r) => r.ok).catch(() => false), {
      timeout: 10_000,
    })
    .toBe(true);
}

async function stopBridge(): Promise<void> {
  const b = bridge;
  bridge = undefined;
  if (!b || b.exitCode !== null) return;
  await new Promise<void>((resolve) => {
    b.once("exit", () => resolve());
    b.kill();
  });
}

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  test.skip(
    !existsSync(bridgeBin) || !existsSync(boothBin),
    "build first: cargo build -p player5-bridge --examples",
  );
  port = await freePort();
  // UDP ports base..base+2 on loopback; a block unlikely to be taken.
  portBase = 42_000 + Math.floor(Math.random() * 2_000) * 3;
  booth = spawn(boothBin, [String(portBase)], { stdio: ["ignore", "pipe", "ignore"] });
  await new Promise<void>((resolve, reject) => {
    let text = "";
    booth!.stdout!.on("data", (chunk: Buffer) => {
      text += chunk.toString();
      for (const m of text.matchAll(/^downbeat (\d+) ([\d.]+) ([\d.]+)$/gm)) {
        downbeats.set(Number(m[1]), { unixMs: Number(m[2]), periodMs: Number(m[3]) });
      }
      if (downbeats.size >= 2) resolve();
    });
    booth!.once("exit", () => reject(new Error("fake_booth exited")));
  });
  bridge = startBridge();
  await bridgeUp();
});

test.afterAll(async () => {
  await stopBridge();
  booth?.kill();
});

test.afterEach(({ page }) => expectClean(page));

/** Heard beat minus the device's beat at the same instant, folded into −2..2. */
async function barError(page: Page, device: number): Promise<number> {
  const db = downbeats.get(device)!;
  const s = await page.evaluate(() => {
    const clock = document.getElementById("clock")!;
    return {
      beat: Number(clock.dataset.beat),
      at: Number(clock.dataset.beatAt),
      origin: performance.timeOrigin,
    };
  });
  const expected = (s.origin + s.at - db.unixMs) / db.periodMs;
  const d = (((s.beat - expected) % 4) + 6) % 4 - 2;
  return d;
}

/** Worst bar error over `n` samples 100 ms apart. */
async function worstBarError(page: Page, device: number, n = 10): Promise<number> {
  let worst = 0;
  for (let i = 0; i < n; i++) {
    worst = Math.max(worst, Math.abs(await barError(page, device)));
    await page.waitForTimeout(100);
  }
  return worst;
}

async function expectAlignedTo(page: Page, device: number): Promise<void> {
  // Two followers in series (bridge, then page) each confirm a phase jump
  // (a new device, a restart) before re-syncing, so allow some seconds;
  // then the heard bar must stay within 1/20 of a beat (24 ms at 125 BPM)
  // of the device's own bar for a whole second.
  await expect
    .poll(() => worstBarError(page, device), { timeout: 20_000, intervals: [0] })
    .toBeLessThan(0.05);
}

test("Pro DJ Link through the real bridge: lock, tempo, bar, follow, restart", async ({ page }) => {
  test.setTimeout(90_000);
  const errors = guard(page, [/WebSocket connection to .* failed/]);
  await page.goto(`http://127.0.0.1:${port}/`);
  await play(page);
  await chooseSource(page, "Bridge");
  await page.getByRole("button", { name: "Connect" }).click();
  const clock = page.locator("#clock");
  await expect(clock).toHaveAttribute("data-bridge", "open");

  // Both players listed; the master target resolves to the lowest-numbered
  // playing player (no status packets here): device 2 at 125 BPM.
  await expect(page.locator("#bridge-devices li")).toHaveCount(2, { timeout: 10_000 });
  await expect(clock).toHaveAttribute("data-lock", "locked", { timeout: 10_000 });
  await expect.poll(async () => Math.abs((await engineTempo(page)) - 125), { timeout: 10_000 }).toBeLessThan(0.05);
  await expect(page.locator("#source-status")).toContainText("device 2");
  await expectAlignedTo(page, 2);

  // Follow device 3 (128 BPM, its own bar phase). Another device is taken
  // at once (its next beat packet), not confirmed like a phase jump first.
  const switched = Date.now();
  await page.locator("#bridge-follow").selectOption("3");
  await expect.poll(async () => Math.abs(await barError(page, 3)), { timeout: 10_000, intervals: [50] }).toBeLessThan(0.05);
  const took = Date.now() - switched;
  expect(took, "ms until the heard bar is device 3's").toBeLessThan(900);
  await expect(page.locator("#source-status")).toContainText("device 3", { timeout: 10_000 });
  await expect.poll(async () => Math.abs((await engineTempo(page)) - 128), { timeout: 15_000 }).toBeLessThan(0.05);
  await expect(clock).toHaveAttribute("data-lock", "locked", { timeout: 10_000 });
  await expectAlignedTo(page, 3);

  // The bridge restarts (it comes back following the master): the page
  // reconnects on its own and asks for device 3 again.
  await stopBridge();
  await expect(clock).toHaveAttribute("data-bridge", /closed|connecting/, { timeout: 10_000 });
  bridge = startBridge();
  await bridgeUp();
  // Through the reconnect the heard bar stays on device 3: a restarted
  // bridge reports the master (device 2) until our follow arrives, and the
  // page must not chase that other deck's bar meanwhile. The menu keeps
  // showing device 3 even while the device list is incomplete.
  let worst = 0;
  for (let i = 0; i < 40; i++) {
    const e3 = await barError(page, 3);
    worst = Math.max(worst, Math.abs(e3));
    await expect(page.locator("#bridge-follow")).toHaveValue("3", { timeout: 0 });
    await page.waitForTimeout(100);
  }
  expect(worst, "largest bar error vs device 3 across the restart").toBeLessThan(0.05);
  await expect(clock).toHaveAttribute("data-bridge", "open", { timeout: 15_000 });
  await expect(page.locator("#bridge-follow")).toHaveValue("3", { timeout: 10_000 });
  await expect(page.locator("#source-status")).toContainText("device 3", { timeout: 10_000 });
  await expect(clock).toHaveAttribute("data-lock", "locked", { timeout: 10_000 });
  await expect.poll(async () => Math.abs((await engineTempo(page)) - 128), { timeout: 15_000 }).toBeLessThan(0.05);
  await expectAlignedTo(page, 3);
  expect(errors).toEqual([]);
});

test("a page from another origin cannot drive the bridge; the app explains why", async ({ page }) => {
  // A non-browser client (no Origin header) follows device 3.
  const timelines: { device: number | null }[] = [];
  const owner = new WebSocket(`ws://127.0.0.1:${port}/ws`);
  owner.onmessage = (e) => {
    const m = JSON.parse(String(e.data)) as { type: string; device: number | null };
    if (m.type === "timeline") timelines.push(m);
  };
  await new Promise<void>((resolve) => (owner.onopen = () => resolve()));
  owner.send(JSON.stringify({ type: "follow", target: 3 }));
  await expect.poll(() => timelines.at(-1)?.device, { timeout: 10_000 }).toBe(3);

  // Any web page open on the DJ laptop tries to switch the booth to device 2.
  await page.route("http://foreign.test/**", (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>elsewhere</title>" }),
  );
  await page.goto("http://foreign.test/");
  const closed = await page.evaluate(async (url) => {
    const ws = new WebSocket(url);
    ws.onopen = () => ws.send(JSON.stringify({ type: "follow", target: 2 }));
    return new Promise<{ code: number; reason: string }>((resolve) => {
      ws.onclose = (e) => resolve({ code: e.code, reason: e.reason });
    });
  }, `ws://127.0.0.1:${port}/ws`);
  expect(closed.code).toBe(1008);
  expect(closed.reason).toContain("--allow-origin");
  await page.waitForTimeout(1_000);
  expect(timelines.at(-1)?.device, "the bridge still follows device 3").toBe(3);
  owner.close();

  // The app itself, hosted on another origin, says what to do.
  await page.route("http://player5.test/**", async (route) => {
    const url = new URL(route.request().url());
    const response = await route.fetch({ url: `http://127.0.0.1:4173${url.pathname}${url.search}` });
    await route.fulfill({ response });
  });
  guard(page, [/WebSocket connection to .* failed/]);
  await page.goto("http://player5.test/");
  await chooseSource(page, "Bridge");
  await page.locator("#bridge-url").fill(`ws://127.0.0.1:${port}/ws`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.locator("#source-status")).toContainText("--allow-origin http://player5.test", {
    timeout: 10_000,
  });
});
