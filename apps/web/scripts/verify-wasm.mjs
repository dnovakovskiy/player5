// Proves the browser build is the same instrument as the native one: renders
// every pattern in patterns/ through public/player5.wasm exactly the way
// engine::spec::PatternSpec::render does, and compares the 16-bit PCM hash
// with the golden masters that CI checks on Linux and macOS.
//
//   npm run verify-wasm      (after scripts/build-wasm.sh)

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, basename } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../../..");
const wasmPath = join(here, "../public/player5.wasm");
const patternsDir = join(root, "patterns");
const goldenDir = join(root, "core/render/tests/golden");

const STEP_COUNT = 16;
const module = new WebAssembly.Module(readFileSync(wasmPath));

function renderSpec(spec) {
  const r = spec.render ?? {};
  const bars = r.bars ?? 2;
  const sampleRate = r.sample_rate ?? 48000;
  const tail = r.tail_seconds ?? 0.5;
  const blockSize = r.block_size ?? 256;
  const bpm = spec.bpm ?? 120;
  const beats = bars * STEP_COUNT * 0.25;
  const frames = Math.round((beats * sampleRate * 60) / bpm + tail * sampleRate);

  const { exports: api } = new WebAssembly.Instance(module, {});
  const engine = api.p5_engine_new(sampleRate);
  const json = new TextEncoder().encode(JSON.stringify(spec) + "\0");
  const jsonPtr = api.p5_alloc(json.length);
  new Uint8Array(api.memory.buffer, jsonPtr, json.length).set(json);
  const rc = api.p5_engine_load_pattern_json(engine, jsonPtr);
  api.p5_free(jsonPtr, json.length);
  if (rc !== 0) throw new Error(`load_pattern_json returned ${rc}`);
  api.p5_engine_set_stop_after(engine, BigInt(bars * STEP_COUNT));
  api.p5_engine_start(engine);

  const out = new Float32Array(frames);
  const bufPtr = api.p5_alloc(blockSize * 4);
  for (let pos = 0; pos < frames; pos += blockSize) {
    const n = Math.min(blockSize, frames - pos);
    api.p5_engine_render(engine, bufPtr, n);
    out.set(new Float32Array(api.memory.buffer, bufPtr, n), pos);
  }
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

let failed = 0;
for (const file of readdirSync(patternsDir).filter((f) => f.endsWith(".json")).sort()) {
  const spec = JSON.parse(readFileSync(join(patternsDir, file), "utf8"));
  const golden = JSON.parse(readFileSync(join(goldenDir, basename(file)), "utf8"));
  const hash = fnv1a64(renderSpec(spec));
  const ok = hash === golden.hash_pcm16_fnv1a64;
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"} ${file}  wasm ${hash}  golden ${golden.hash_pcm16_fnv1a64}`);
}
if (failed) {
  console.error(`${failed} pattern(s) differ between wasm and native`);
  process.exit(1);
}
console.log("wasm render is bit-identical to the native golden masters");
