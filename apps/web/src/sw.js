// player5 service worker (template). vite.player5.ts fills in the version
// and the precache list at build time and emits it as dist/sw.js.
//
//   * install: precache the built shell into a cache named by build version
//   * activate: drop caches of older versions, take control of open pages
//   * navigations: network first (fresh deploys win), cached shell offline
//   * hashed assets (assets/*) and other precached files: cache first
//   * anything else (bridge.json, cross-origin): straight to the network

/* global self, caches */

const VERSION = "__P5_VERSION__";
const PRECACHE = __P5_PRECACHE__;
const CACHE = "player5-" + VERSION;
const SHELL = "./";

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE.map((url) => new Request(url, { cache: "reload" }))))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k.startsWith("player5-") && k !== CACHE).map((k) => caches.delete(k))),
      )
      .then(() => self.clients.claim()),
  );
});

const MATCH = { ignoreSearch: true, ignoreVary: true };
/** A navigation waits this long for the network before using the cached shell. */
const NETWORK_TIMEOUT_MS = 4000;

async function networkFirst(request) {
  const cache = await caches.open(CACHE);
  const fromCache = async () => (await cache.match(request, MATCH)) || (await cache.match(SHELL, MATCH));
  let timer;
  const network = fetch(request).then((response) => {
    if (response.ok) cache.put(SHELL, response.clone());
    return response;
  });
  network.catch(() => {}); // handled below; avoid an unhandled rejection when the timeout wins
  const timeout = new Promise((resolve) => {
    timer = setTimeout(resolve, NETWORK_TIMEOUT_MS, null);
  });
  try {
    const response = await Promise.race([network, timeout]);
    if (response) return response;
    return (await fromCache()) || (await network);
  } catch (err) {
    const cached = await fromCache();
    if (cached) return cached;
    throw err;
  } finally {
    clearTimeout(timer);
  }
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const cached = await cache.match(request, MATCH);
  if (cached) return cached;
  const response = await fetch(request);
  if (response.ok) cache.put(request, response.clone());
  return response;
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request));
    return;
  }
  const scope = new URL(self.registration.scope);
  const path = url.pathname.startsWith(scope.pathname) ? url.pathname.slice(scope.pathname.length) : null;
  if (path !== null && (path.startsWith("assets/") || PRECACHE.includes(path))) {
    event.respondWith(cacheFirst(request));
  }
});
