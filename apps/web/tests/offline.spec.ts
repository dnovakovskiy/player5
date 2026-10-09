import { expect, test } from "@playwright/test";
import { expectClean, expectPlayheadAdvances, guard, play } from "./support/helpers";

test.afterEach(({ page }) => expectClean(page));

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
  // The core ships content-hashed, never under a stable name a cache could
  // pair with another build's JavaScript.
  expect(cached.some((p) => /\/assets\/player5-[\w-]+\.wasm$/.test(p))).toBe(true);
  expect(cached).not.toContain("/player5.wasm");
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

test("service worker: a new deploy wins, other pages never replace the shell", async ({ page, context }) => {
  guard(page);
  await page.goto("/");
  await page.waitForFunction(() => navigator.serviceWorker.controller !== null, null, { timeout: 15_000 });

  // The next deploy's index.html (simulated on the wire): navigations are
  // network first, so the controlled page gets it, through the worker.
  const root = (url: URL) => url.pathname === "/";
  await context.route(root, async (route) => {
    const res = await route.fetch();
    const body = (await res.text()).replace("<title>player5</title>", "<title>player5 next</title>");
    await route.fulfill({ response: res, body });
  });
  const next = await page.reload();
  expect(next?.fromServiceWorker()).toBe(true);
  await expect(page).toHaveTitle("player5 next");
  await context.unroute(root);

  // Another page in the worker's scope (pages.yml publishes
  // player5-standalone.html next to the app) must not become the offline
  // shell.
  await context.route("**/other-page.html", (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>other page</title>" }),
  );
  await page.goto("/other-page.html");
  await expect(page).toHaveTitle("other page");
  await context.setOffline(true);
  try {
    // The offline shell is the newest index.html the network served.
    await page.goto("/#p=" + "eyJicG0iOjEyNn0");
    await expect(page).toHaveTitle("player5 next");
    await expect(page.locator("#bpm")).toHaveValue("126");
    await expect(page.getByRole("button", { name: "Play", exact: true })).toBeVisible();
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
