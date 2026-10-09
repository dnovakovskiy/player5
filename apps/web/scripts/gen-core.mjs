// Builds the pure-JavaScript copy of the core: public/player5.wasm run
// through Binaryen's wasm2js. The browser falls back to it when a page's
// Content-Security-Policy forbids compiling WebAssembly (no
// 'wasm-unsafe-eval'), e.g. inside a sandboxed viewer (ADR-0010). It is the
// same instrument: scripts/verify-wasm.mjs renders every golden pattern
// through it and requires bit-identical output.
//
//   node scripts/gen-core.mjs [--force]
//
// Output: src/generated/core.js (gitignored), an ES module exporting
// `createCore()`, which returns an object shaped like a WebAssembly
// instance's exports (`memory.buffer`, `p5_*` functions). Regenerated
// whenever the wasm file is newer than the output.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const wasmPath = join(here, "../public/player5.wasm");
export const corePath = join(here, "../src/generated/core.js");

// Only the lockstep engine API the browser host calls. Everything else
// (the split API for native audio threads, i64-taking functions) is
// dropped before translation, which keeps the JS small and avoids i64
// legalisation at the boundary.
export const KEEP_EXPORTS = [
  "memory",
  "p5_abi_version",
  "p5_alloc",
  "p5_free",
  "p5_engine_new",
  "p5_engine_free",
  "p5_engine_load_pattern_json",
  "p5_engine_start",
  "p5_engine_stop",
  "p5_engine_set_stop_after_u32",
  "p5_engine_position_f64",
  "p5_engine_playing_step",
  "p5_engine_tempo",
  "p5_engine_beat",
  "p5_engine_clock_locked",
  "p5_engine_set_clock_mode",
  "p5_engine_observe",
  "p5_engine_midi",
  "p5_engine_tap",
  "p5_engine_resync",
  "p5_engine_set_nudge_ms",
  "p5_engine_set_latency_ms",
  "p5_engine_render",
];

function binaryenDir() {
  return dirname(fileURLToPath(import.meta.resolve("binaryen")));
}

/** True when core.js is missing or older than the wasm module. */
export function coreIsStale() {
  if (!existsSync(corePath)) return true;
  if (!existsSync(wasmPath)) return false;
  return statSync(wasmPath).mtimeMs > statSync(corePath).mtimeMs;
}

/** Generates src/generated/core.js if stale (or always with `force`). */
export async function generateCore({ force = false, quiet = false } = {}) {
  if (!force && !coreIsStale()) return corePath;
  if (!existsSync(wasmPath)) {
    throw new Error(`${wasmPath} is missing: run scripts/build-wasm.sh first`);
  }
  const { default: binaryen } = await import("binaryen");
  const module = binaryen.readBinary(new Uint8Array(readFileSync(wasmPath)));
  // rustc's wasm32 target emits bulk-memory, non-trapping float-to-int and
  // sign-extension ops; enable them for parsing, then lower them away so
  // wasm2js (MVP only) can translate. The lowerings are exact.
  const F = binaryen.Features;
  module.setFeatures(
    F.MVP | F.MutableGlobals | F.BulkMemory | F.BulkMemoryOpt | F.NontrappingFPToInt | F.SignExt |
      F.Multivalue | F.ReferenceTypes,
  );
  const exported = [];
  for (let i = 0; i < module.getNumExports(); i++) {
    exported.push(binaryen.getExportInfo(module.getExportByIndex(i)).name);
  }
  for (const name of KEEP_EXPORTS) {
    if (!exported.includes(name)) throw new Error(`player5.wasm does not export ${name}`);
  }
  for (const name of exported) if (!KEEP_EXPORTS.includes(name)) module.removeExport(name);
  module.runPasses([
    "llvm-memory-copy-fill-lowering",
    "llvm-nontrapping-fptoint-lowering",
    "signext-lowering",
    "remove-unused-module-elements",
  ]);
  module.setFeatures(F.MVP | F.MutableGlobals);
  if (!module.validate()) throw new Error("lowered module failed validation");
  const lowered = module.emitBinary();
  module.dispose();

  const tmp = mkdtempSync(join(tmpdir(), "player5-core-"));
  try {
    const input = join(tmp, "core.wasm");
    const output = join(tmp, "core.js");
    writeFileSync(input, lowered);
    execFileSync(process.execPath, [join(binaryenDir(), "bin/wasm2js"), input, "-O2", "-o", output], {
      stdio: quiet ? "ignore" : ["ignore", "ignore", "inherit"],
    });
    const js = wrap(readFileSync(output, "utf8"));
    mkdirSync(dirname(corePath), { recursive: true });
    writeFileSync(corePath, js);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  if (!quiet) console.log(`generated ${corePath} (${(statSync(corePath).size / 1024).toFixed(0)} KiB)`);
  return corePath;
}

/**
 * wasm2js emits a module that imports `env` and instantiates itself once at
 * load. Turn that into a factory so every caller gets a fresh instance
 * (own memory), like `new WebAssembly.Instance`.
 */
function wrap(src) {
  const body = src
    .replace(/^import \* as env from 'env';\s*$/m, "")
    .replace(/\nvar retasmFunc = asmFunc\([\s\S]*$/, "\n");
  if (body.includes("retasmFunc") || !body.includes("function asmFunc(")) {
    throw new Error("unexpected wasm2js output shape");
  }
  return (
    "// GENERATED by scripts/gen-core.mjs from public/player5.wasm. Do not edit.\n" +
    "/* eslint-disable */\n" +
    body +
    "\n/** A fresh instance of the core; same exports as the wasm module. */\n" +
    "export function createCore() {\n" +
    "  return asmFunc({ env: { setTempRet0: function () {} } });\n" +
    "}\n"
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  await generateCore({ force: process.argv.includes("--force") });
}
