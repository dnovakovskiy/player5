// The pattern file format shared with the Rust core
// (core/engine/src/spec.rs), the #p= URL-hash codec and the "pattern code"
// used where URLs cannot carry state.
//
// In the app every voice is always present (`PatternSpec`). On the wire
// (URL hash, pattern code, the JSON the core loads) the spec is compacted:
// voices that are silent with default controls are omitted and controls at
// their default are left out, which the core fills back in identically.

import { CONTROL_DEFAULTS, CONTROL_IDS, VOICE_IDS, type ControlId, type VoiceId } from "./voices";

export const STEP_COUNT = 16;

/** Step symbols: off, hit, accent, flam, accent + flam. */
export type StepChar = "-" | "x" | "X" | "f" | "F";

export interface VoiceSpec {
  steps: string;
  tune: number;
  decay: number;
  tone: number;
  snappy: number;
  level: number;
  mute: boolean;
}

export interface RenderSpec {
  output_gain: number;
  limiter: boolean;
}

export interface PatternSpec {
  bpm: number;
  shuffle: number;
  accent: number;
  flam: number;
  voices: Record<VoiceId, VoiceSpec>;
  render: RenderSpec;
}

export const EMPTY_STEPS = "-".repeat(STEP_COUNT);
export const BPM_MIN = 20;
export const BPM_MAX = 400;
export const GAIN_MAX = 4;

export function defaultVoice(steps = EMPTY_STEPS): VoiceSpec {
  return { steps, ...CONTROL_DEFAULTS, mute: false };
}

/** A blank pattern: no steps, core defaults everywhere. */
export function emptySpec(bpm = 124): PatternSpec {
  const voices = {} as Record<VoiceId, VoiceSpec>;
  for (const id of VOICE_IDS) voices[id] = defaultVoice();
  return {
    bpm,
    shuffle: 0,
    accent: 0.5,
    flam: 0.5,
    voices,
    render: { output_gain: 1, limiter: false },
  };
}

export function cloneSpec(spec: PatternSpec): PatternSpec {
  return JSON.parse(JSON.stringify(spec)) as PatternSpec;
}

// ---- steps ----

/** Strips grouping spaces, maps '.' to '-', drops unknown symbols, pads/truncates to 16. */
export function normalizeSteps(steps: string): string {
  const s = steps
    .replace(/\s+/g, "")
    .replace(/[^-xXfF.]/g, "-")
    .replace(/\./g, "-");
  return (s + EMPTY_STEPS).slice(0, STEP_COUNT);
}

export function stepAt(steps: string, index: number): StepChar {
  return (normalizeSteps(steps)[index] ?? "-") as StepChar;
}

function withStep(steps: string, index: number, c: StepChar): string {
  const chars = [...normalizeSteps(steps)];
  chars[index] = c;
  return chars.join("");
}

/** Normal tap: off → hit → accent → off. A flammed step stays flammed. */
export function cycleStep(steps: string, index: number): string {
  const next: Record<StepChar, StepChar> = { "-": "x", x: "X", X: "-", f: "F", F: "-" };
  return withStep(steps, index, next[stepAt(steps, index)]);
}

/** Flam-mode tap: toggles the flam on a step (an empty step becomes a flammed hit). */
export function toggleFlam(steps: string, index: number): string {
  const next: Record<StepChar, StepChar> = { "-": "f", x: "f", X: "F", f: "x", F: "X" };
  return withStep(steps, index, next[stepAt(steps, index)]);
}

export function describeStep(c: StepChar): string {
  return { "-": "off", x: "hit", X: "accent", f: "flam", F: "accent flam" }[c];
}

// ---- sanitising ----

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const round = (v: number, places: number) => {
  const k = 10 ** places;
  return Math.round(v * k) / k;
};
const num = (v: unknown, fallback: number): number => {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isFinite(n) ? n : fallback;
};
const unit = (v: unknown, fallback: number) => round(clamp(num(v, fallback), 0, 1), 3);

/**
 * Brings any parsed object into range. Unknown fields are dropped and
 * missing ones take the core's defaults, so old links (kick only, no flam)
 * load unchanged.
 */
export function sanitize(input: unknown): PatternSpec {
  const d = emptySpec();
  if (!input || typeof input !== "object") return d;
  const o = input as Record<string, unknown>;
  const voicesIn = (o.voices && typeof o.voices === "object" ? o.voices : {}) as Record<string, unknown>;
  const renderIn = (o.render && typeof o.render === "object" ? o.render : {}) as Record<string, unknown>;
  const voices = {} as Record<VoiceId, VoiceSpec>;
  for (const id of VOICE_IDS) {
    const v = (voicesIn[id] && typeof voicesIn[id] === "object" ? voicesIn[id] : {}) as Record<string, unknown>;
    const out = defaultVoice(normalizeSteps(typeof v.steps === "string" ? v.steps : EMPTY_STEPS));
    for (const c of CONTROL_IDS) out[c] = unit(v[c], CONTROL_DEFAULTS[c]);
    out.mute = v.mute === true;
    voices[id] = out;
  }
  return {
    bpm: round(clamp(num(o.bpm, 120), BPM_MIN, BPM_MAX), 2),
    shuffle: unit(o.shuffle, 0),
    accent: unit(o.accent, 0.5),
    flam: unit(o.flam, 0.5),
    voices,
    render: {
      output_gain: round(clamp(num(renderIn.output_gain, 1), 0, GAIN_MAX), 4),
      limiter: renderIn.limiter === true,
    },
  };
}

// ---- wire format ----

type WireVoice = { steps: string } & Partial<Record<ControlId, number>> & { mute?: true };

/** The compact JSON object the core and the URL carry. */
export function toWire(spec: PatternSpec): Record<string, unknown> {
  const voices: Record<string, WireVoice> = {};
  for (const id of VOICE_IDS) {
    const v = spec.voices[id];
    const w: WireVoice = { steps: normalizeSteps(v.steps) };
    let interesting = w.steps !== EMPTY_STEPS || v.mute;
    for (const c of CONTROL_IDS) {
      if (v[c] !== CONTROL_DEFAULTS[c]) {
        w[c] = v[c];
        interesting = true;
      }
    }
    if (v.mute) w.mute = true;
    if (interesting) voices[id] = w;
  }
  const wire: Record<string, unknown> = {
    bpm: spec.bpm,
    shuffle: spec.shuffle,
    accent: spec.accent,
    flam: spec.flam,
    voices,
  };
  if (spec.render.output_gain !== 1 || spec.render.limiter) {
    wire.render = { output_gain: spec.render.output_gain, limiter: spec.render.limiter };
  }
  return wire;
}

export function toJson(spec: PatternSpec): string {
  return JSON.stringify(toWire(spec));
}

/** JSON the core accepts, as NUL-terminated UTF-8 for the engine host. */
export function specBytes(spec: PatternSpec): Uint8Array {
  const body = new TextEncoder().encode(toJson(spec));
  const out = new Uint8Array(body.length + 1);
  out.set(body);
  return out;
}

// ---- URL hash and pattern code: base64url(JSON) ----

function toBase64Url(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(s: string): string {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (s.length % 4)) % 4);
  const bin = atob(b64);
  const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

/** The pattern code: the base64url payload of a #p= link. */
export function encodeCode(spec: PatternSpec): string {
  return toBase64Url(toJson(spec));
}

export function decodeCode(code: string): PatternSpec | null {
  if (!/^[A-Za-z0-9_-]+$/.test(code)) return null;
  try {
    const parsed: unknown = JSON.parse(fromBase64Url(code));
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    return sanitize(parsed);
  } catch {
    return null;
  }
}

export function encodeHash(spec: PatternSpec): string {
  return "#p=" + encodeCode(spec);
}

export function decodeHash(hash: string): PatternSpec | null {
  const m = /^#p=([A-Za-z0-9_-]+)$/.exec(hash);
  return m ? decodeCode(m[1]!) : null;
}

/**
 * Accepts what people paste: a bare pattern code, "#p=<code>", or a whole
 * link containing "#p=<code>". Whitespace (line wraps in chat apps) is
 * ignored.
 */
export function parsePatternInput(text: string): PatternSpec | null {
  const t = text.replace(/\s+/g, "");
  if (!t) return null;
  const m = /#p=([A-Za-z0-9_-]+)/.exec(t);
  return decodeCode(m ? m[1]! : t.replace(/^p=/, ""));
}
