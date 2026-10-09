// Build plugin for apps/web (ADR-0010).
//
// Normal build (`vite build` → dist/):
//   * virtual:player5/core-js → src/generated/core.js, regenerated from
//     public/player5.wasm with wasm2js when stale (a lazy chunk; loaded only
//     when WebAssembly is blocked);
//   * PNG app icons drawn by scripts/icons.mjs;
//   * dist/sw.js from src/sw.js with a versioned cache name and the
//     precache list of everything the build emitted.
//
// Single-file build (`vite build --mode single` → dist-single/player5.html):
//   * virtual:player5/wasm → the wasm module as base64;
//   * JS, CSS, worklet source and the JS core inlined into one HTML file;
//     no manifest, no icons, no service worker, nothing fetched at runtime.
//   * The build fails if the output could make a network request.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import type { Connect, Plugin, ResolvedConfig } from "vite";
// @ts-expect-error -- plain .mjs build script without type declarations
import { corePath, generateCore, wasmPath } from "./scripts/gen-core.mjs";
// @ts-expect-error -- plain .mjs build script without type declarations
import { iconSet } from "./scripts/icons.mjs";

const WASM_ID = "virtual:player5/wasm";
const CORE_ID = "virtual:player5/core-js";

/** Strings that would mean a network request in the single-file output. */
export const FORBIDDEN_IN_SINGLE = ["http:", "https:", "fetch(", "importScripts", "XMLHttpRequest", "sendBeacon"];

function listFiles(dir: string, base = dir): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? listFiles(path, base) : [relative(base, path).split("\\").join("/")];
  });
}

export function player5(options: { single: boolean }): Plugin[] {
  const { single } = options;
  let config: ResolvedConfig;

  const modules: Plugin = {
    name: "player5:modules",
    enforce: "pre",
    configResolved(c) {
      config = c;
    },
    async resolveId(id) {
      if (id === WASM_ID) return "\0" + WASM_ID;
      if (id === CORE_ID) {
        await generateCore({ quiet: true });
        return corePath as string;
      }
      return null;
    },
    load(id) {
      if (id !== "\0" + WASM_ID) return null;
      if (!single) return "export default null;";
      if (!existsSync(wasmPath)) this.error(`${wasmPath} is missing: run scripts/build-wasm.sh first`);
      this.addWatchFile(wasmPath);
      return `export default ${JSON.stringify(readFileSync(wasmPath).toString("base64"))};`;
    },
  };

  const icons: Plugin = {
    name: "player5:icons",
    apply: () => !single,
    configureServer(server) {
      let cache: Record<string, Buffer> | null = null;
      const middleware: Connect.NextHandleFunction = (req, res, next) => {
        const path = (req.url ?? "").split("?")[0]!.replace(/^\//, "");
        if (!path.startsWith("icons/")) return next();
        cache ??= iconSet() as Record<string, Buffer>;
        const png = cache[path];
        if (!png) return next();
        res.setHeader("Content-Type", "image/png");
        res.end(png);
      };
      server.middlewares.use(middleware);
    },
    generateBundle() {
      for (const [fileName, source] of Object.entries(iconSet() as Record<string, Buffer>)) {
        this.emitFile({ type: "asset", fileName, source });
      }
    },
  };

  const serviceWorker: Plugin = {
    name: "player5:service-worker",
    apply: (_c, env) => env.command === "build" && !single,
    enforce: "post",
    generateBundle: {
      order: "post",
      handler(_opts, bundle) {
        const files = new Map<string, Uint8Array | string>();
        for (const [name, item] of Object.entries(bundle)) {
          if (name.endsWith(".map")) continue;
          files.set(name, item.type === "chunk" ? item.code : item.source);
        }
        // Vite copies public/ separately; precache those files too.
        const publicDir = config.publicDir;
        for (const name of listFiles(publicDir)) {
          if (!files.has(name)) files.set(name, readFileSync(join(publicDir, name)));
        }
        const names = [...files.keys()].filter((n) => n !== "sw.js").sort();
        const hash = createHash("sha256");
        for (const n of names) hash.update(n).update("\0").update(files.get(n)!).update("\0");
        const version = hash.digest("hex").slice(0, 16);
        const precache = ["./", ...names];
        const source = readFileSync(join(config.root, "src/sw.js"), "utf8")
          .replace("__P5_VERSION__", version)
          .replace("__P5_PRECACHE__", JSON.stringify(precache));
        this.emitFile({ type: "asset", fileName: "sw.js", source });
      },
    },
  };

  const singleFile: Plugin = {
    name: "player5:single-file",
    apply: (_c, env) => env.command === "build" && single,
    enforce: "post",
    transformIndexHtml(html) {
      // No manifest, icons or theme links: nothing may be fetched.
      return html
        .replace(/\s*<link rel="manifest"[^>]*>/g, "")
        .replace(/\s*<link rel="(?:icon|apple-touch-icon)"[^>]*>/g, "");
    },
    generateBundle: {
      order: "post",
      handler(_opts, bundle) {
        const html = bundle["index.html"];
        if (!html || html.type !== "asset") return this.error("index.html missing from the bundle");
        let out = String(html.source);
        const inlined = new Set<string>(["index.html"]);
        out = out.replace(/<script type="module" crossorigin src="\.\/([^"]+)"><\/script>/g, (_m, file: string) => {
          const chunk = bundle[file];
          if (!chunk || chunk.type !== "chunk") return this.error(`script ${file} not in bundle`);
          inlined.add(file);
          if (chunk.code.includes("<!--")) this.error("inlined JS contains '<!--'");
          return `<script type="module">${chunk.code.replace(/<\/(script)/gi, "<\\/$1")}</script>`;
        });
        out = out.replace(/<link rel="stylesheet" crossorigin href="\.\/([^"]+)">/g, (_m, file: string) => {
          const asset = bundle[file];
          if (!asset || asset.type !== "asset") return this.error(`stylesheet ${file} not in bundle`);
          inlined.add(file);
          return `<style>${String(asset.source).replace(/<\/(style)/gi, "<\\/$1")}</style>`;
        });
        for (const name of Object.keys(bundle)) {
          if (!inlined.has(name) && !name.endsWith(".map")) {
            this.error(`single-file build would need a separate file: ${name}`);
          }
          delete bundle[name];
        }
        for (const bad of FORBIDDEN_IN_SINGLE) {
          if (out.includes(bad)) this.error(`single-file output contains "${bad}"`);
        }
        const titleAt = out.indexOf("<title>player5</title>");
        if (titleAt < 0 || titleAt > 8192) this.error("<title> must be within the first 8 KB");
        this.emitFile({ type: "asset", fileName: "player5.html", source: out });
      },
    },
  };

  return [modules, icons, serviceWorker, singleFile];
}
