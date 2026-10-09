// Installable, offline-capable PWA (normal build only; the single-file
// build has neither a service worker nor a manifest). The service worker
// is generated at build time from src/sw.js with the precache list and a
// versioned cache name (see vite.player5.ts).

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

export function setupPwa(installButton: HTMLButtonElement): void {
  if (__P5_SINGLE__) return;
  if (import.meta.env.PROD && "serviceWorker" in navigator && /^https?:$/.test(location.protocol)) {
    window.addEventListener("load", () => {
      navigator.serviceWorker
        .register("sw.js", { scope: "./", updateViaCache: "none" })
        .catch((err: unknown) => console.warn("player5: service worker not registered:", err));
    });
  }

  let deferred: BeforeInstallPromptEvent | null = null;
  window.addEventListener("beforeinstallprompt", (e) => {
    e.preventDefault();
    deferred = e as BeforeInstallPromptEvent;
    installButton.hidden = false;
  });
  installButton.addEventListener("click", async () => {
    const installEvent = deferred;
    deferred = null;
    installButton.hidden = true;
    if (!installEvent) return;
    try {
      await installEvent.prompt();
      await installEvent.userChoice;
    } catch {
      /* dismissed or not allowed */
    }
  });
  window.addEventListener("appinstalled", () => {
    deferred = null;
    installButton.hidden = true;
  });
}
