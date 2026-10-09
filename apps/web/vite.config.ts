import { defineConfig } from "vite";
import { player5 } from "./vite.player5";

// Two builds from one source (ADR-0010):
//   vite build                → dist/: the PWA (static hosting, relative
//                               paths, service worker, manifest, icons)
//   vite build --mode single  → dist-single/player5.html: one self-contained
//                               file for sandboxed viewers
export default defineConfig(({ mode }) => {
  const single = mode === "single";
  return {
    base: "./",
    publicDir: single ? false : "public",
    define: {
      __P5_SINGLE__: JSON.stringify(single),
    },
    plugins: player5({ single }),
    build: single
      ? {
          target: "es2022",
          outDir: "dist-single",
          emptyOutDir: true,
          sourcemap: false,
          cssCodeSplit: false,
          assetsInlineLimit: Number.MAX_SAFE_INTEGER,
          modulePreload: false,
          rollupOptions: { output: { inlineDynamicImports: true } },
        }
      : {
          target: "es2022",
          sourcemap: true,
        },
    server: { port: 5173, strictPort: true },
    // booth.test: tests/insecure.spec.ts maps it to 127.0.0.1 to load the
    // app from an insecure (non-loopback) origin, like apps/bridge --web.
    preview: { host: "127.0.0.1", port: 4173, strictPort: true, allowedHosts: ["booth.test"] },
  };
});
