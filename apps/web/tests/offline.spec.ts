import { expect, test } from "@playwright/test";
import { expectPlayheadAdvances, guard, play } from "./support/helpers";

test("service worker: works offline after the first visit", async ({ page, context }) => {
  guard(page);
  await page.goto("/");
  // Installed, activated and controlling this page (clients.claim()).
  await page.evaluate(async () => {
    await navigator.serviceWorker.ready;
  });
  await page.waitForFunction(() => navigator.serviceWorker.controller !== null, null, { timeout: 15_000 });
  const cacheName = await page.evaluate(async () => (await caches.keys()).find((k) => k.startsWith("player5-")));
  expect(cacheName).toMatch(/^player5-[0-9a-f]{16}$/);
  const cached = await page.evaluate(async (name) => {
    const cache = await caches.open(name!);
    return (await cache.keys()).map((r) => new URL(r.url).pathname);
  }, cacheName);
  expect(cached).toContain("/player5.wasm");
  expect(cached).toContain("/icons/icon-192.png");
  expect(cached.some((p) => /\/assets\/core-.*\.js$/.test(p))).toBe(true);

  await context.setOffline(true);
  try {
    await page.reload();
    await expect(page.getByRole("button", { name: "Play", exact: true })).toBeVisible();
    await play(page); // wasm comes from the cache
    await expectPlayheadAdvances(page, 3);
    // A shared link opens offline too (navigation falls back to the shell).
    await page.goto("/?engine=js#p=eyJicG0iOjEzMCwidm9pY2VzIjp7ImtpY2siOnsic3RlcHMiOiJ4LS0teC0tLXgtLS14LS0tIn19fQ");
    await expect(page.locator("#bpm")).toHaveValue("130");
    await play(page); // the lazily loaded JS core is precached as well
    await expectPlayheadAdvances(page, 3);
  } finally {
    await context.setOffline(false);
  }
});

test("manifest and icons are served", async ({ request }) => {
  const manifest = await (await request.get("/manifest.webmanifest")).json();
  expect(manifest.name).toBe("player5");
  const sizes = manifest.icons.map((i: { sizes: string; purpose: string }) => `${i.sizes}:${i.purpose}`);
  expect(sizes).toEqual(expect.arrayContaining(["192x192:any", "512x512:any", "512x512:maskable"]));
  for (const icon of manifest.icons as { src: string; type: string }[]) {
    const res = await request.get(`/${icon.src}`);
    expect(res.ok(), icon.src).toBe(true);
    if (icon.type === "image/png") {
      const body = await res.body();
      expect(body.subarray(1, 4).toString("latin1")).toBe("PNG");
    }
  }
  const sw = await (await request.get("/sw.js")).text();
  expect(sw).not.toContain("__P5_");
});
