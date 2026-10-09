// Proves the browser builds are the same instrument as the native one:
// renders every pattern in patterns/ through BOTH browser builds of the
// core, exactly the way engine::spec::PatternSpec::render does, and
// compares the 16-bit PCM hash with the golden masters that CI checks on
// Linux and macOS:
//
//   * public/player5.wasm   (WebAssembly; AudioWorklet and ScriptProcessor)
//   * src/generated/core.js (the same module through wasm2js; the fallback
//                            for pages whose CSP forbids WebAssembly)
//
//   npm run verify-wasm      (after scripts/build-wasm.sh)
//
// The golden set grows as voices land: this iterates the directory, and a
// pattern without a golden file is an error.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { corePath, generateCore, wasmPath } from "./gen-core.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../../..");
const patternsDir = join(root, "patterns");
const goldenDir = join(root, "core/render/tests/golden");

const STEP_COUNT = 16;

/** Renders a pattern file through one instance of the core's exports. */
function renderSpec(api, spec) {
  const r = spec.render ?? {};
  const bars = r.bars ?? 2;
  const sampleRate = r.sample_rate ?? 48000;
  const tail = r.tail_seconds ?? 0.5;
  const blockSize = r.block_size ?? 256;
  const bpm = spec.bpm ?? 120;
  const beats = bars * STEP_COUNT * 0.25;
  const frames = Math.round((beats * sampleRate * 60) / bpm + tail * sampleRate);

  const engine = api.p5_engine_new(sampleRate);
  const json = new TextEncoder().encode(JSON.stringify(spec) + "\0");
  const jsonPtr = api.p5_alloc(json.length);
  new Uint8Array(api.memory.buffer, jsonPtr, json.length).set(json);
  const rc = api.p5_engine_load_pattern_json(engine, jsonPtr);
  api.p5_free(jsonPtr, json.length);
  if (rc !== 0) throw new Error(`load_pattern_json returned ${rc}`);
  api.p5_engine_set_stop_after_u32(engine, bars * STEP_COUNT);
  api.p5_engine_start(engine);

  const out = new Float32Array(frames);
  const bufPtr = api.p5_alloc(blockSize * 4);
  for (let pos = 0; pos < frames; pos += blockSize) {
    const n = Math.min(blockSize, frames - pos);
    api.p5_engine_render(engine, bufPtr, n);
    // The buffer object changes when memory grows, so re-view per block.
    out.set(new Float32Array(api.memory.buffer, bufPtr, n), pos);
  }
  if (api.p5_engine_position_f64(engine) !== frames) throw new Error("position_f64 mismatch");
  api.p5_free(bufPtr, blockSize * 4);
  api.p5_engine_free(engine);
  return out;
}

// Same conversion as render::to_pcm16: clamp, scale *in f32* (fround of the
// exact double product is the IEEE single multiply), round half away from
// zero.
function toPcm16(x) {
  const v = Math.fround(Math.fround(Math.min(1, Math.max(-1, x))) * 32767);
  return Math.sign(v) * Math.round(Math.abs(v));
}

function fnv1a64(samples) {
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  for (const s of samples) {
    const v = toPcm16(s) & 0xffff;
    for (const b of [v & 0xff, (v >> 8) & 0xff]) {
      h ^= BigInt(b);
      h = (h * prime) & mask;
    }
  }
  return "0x" + h.toString(16).padStart(16, "0");
}

if (!existsSync(wasmPath)) {
  console.error(`${wasmPath} is missing: run scripts/build-wasm.sh first`);
  process.exit(1);
}
await generateCore({ quiet: false });
const wasmModule = new WebAssembly.Module(readFileSync(wasmPath));
const { createCore } = await import(pathToFileURL(corePath).href);

const builds = [
  ["wasm", () => new WebAssembly.Instance(wasmModule, {}).exports],
  ["js  ", () => createCore()],
];

let failed = 0;
let checked = 0;
const files = readdirSync(patternsDir)
  .filter((f) => f.endsWith(".json"))
  .sort();
for (const file of files) {
  const spec = JSON.parse(readFileSync(join(patternsDir, file), "utf8"));
  const goldenPath = join(goldenDir, basename(file));
  if (!existsSync(goldenPath)) {
    console.log(`FAIL ${file}  no golden master at ${goldenPath}`);
    failed++;
    continue;
  }
  const golden = JSON.parse(readFileSync(goldenPath, "utf8")).hash_pcm16_fnv1a64;
  for (const [name, instantiate] of builds) {
    const hash = fnv1a64(renderSpec(instantiate(), spec));
    const ok = hash === golden;
    checked++;
    if (!ok) failed++;
    console.log(`${ok ? "ok  " : "FAIL"} ${name} ${file}  ${hash}  golden ${golden}`);
  }
}
if (files.length === 0) {
  console.error("no patterns found");
  process.exit(1);
}
if (failed) {
  console.error(`${failed} render(s) differ from the native golden masters`);
  process.exit(1);
}
console.log(`${checked} renders: wasm and JS core are bit-identical to the native golden masters`);
