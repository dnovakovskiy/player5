// The pattern file format shared with the Rust core (core/engine/src/spec.rs).

export const STEP_COUNT = 16;

export interface VoiceSpec {
  steps: string;
  tune: number;
  decay: number;
  level: number;
}

export interface RenderSpec {
  output_gain?: number;
  limiter?: boolean;
}

export interface PatternSpec {
  bpm: number;
  shuffle: number;
  accent: number;
  voices: { kick?: VoiceSpec };
  render?: RenderSpec;
}

export function defaultSpec(): PatternSpec {
  return {
    bpm: 124,
    shuffle: 0,
    accent: 1,
    voices: { kick: { steps: "X---x---X---x---", tune: 0.5, decay: 0.5, level: 1 } },
    render: { output_gain: 1, limiter: false },
  };
}

/** Off → hit → accent → off. */
export function cycleStep(steps: string, index: number): string {
  const chars = [...normalizeSteps(steps)];
  const current = chars[index] ?? "-";
  chars[index] = current === "-" ? "x" : current === "x" ? "X" : "-";
  return chars.join("");
}

/** Strips grouping spaces and pads/truncates to 16 characters. */
export function normalizeSteps(steps: string): string {
  const s = steps.replace(/\s+/g, "").replace(/\./g, "-");
  return (s + "-".repeat(STEP_COUNT)).slice(0, STEP_COUNT);
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, Number.isFinite(v) ? v : 0));

/** Brings any parsed object into range; unknown fields are dropped. */
export function sanitize(input: unknown): PatternSpec {
  const d = defaultSpec();
  if (!input || typeof input !== "object") return d;
  const o = input as Record<string, unknown>;
  const voices = (o.voices ?? {}) as Record<string, unknown>;
  const kick = (voices.kick ?? {}) as Record<string, unknown>;
  const render = (o.render ?? {}) as Record<string, unknown>;
  const bpm = Number(o.bpm);
  return {
    bpm: Number.isFinite(bpm) ? Math.min(400, Math.max(20, bpm)) : d.bpm,
    shuffle: clamp01(Number(o.shuffle ?? d.shuffle)),
    accent: clamp01(Number(o.accent ?? d.accent)),
    voices: {
      kick: {
        steps: normalizeSteps(typeof kick.steps === "string" ? kick.steps : d.voices.kick!.steps),
        tune: clamp01(Number(kick.tune ?? 0.5)),
        decay: clamp01(Number(kick.decay ?? 0.5)),
        level: clamp01(Number(kick.level ?? 1)),
      },
    },
    render: {
      output_gain: Math.min(4, Math.max(0, Number(render.output_gain ?? 1) || 0)),
      limiter: Boolean(render.limiter ?? false),
    },
  };
}

// ---- URL hash: #p=<base64url(JSON)> — one link shares a pattern. ----

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

export function encodeHash(spec: PatternSpec): string {
  return "#p=" + toBase64Url(JSON.stringify(spec));
}

export function decodeHash(hash: string): PatternSpec | null {
  const m = /^#p=([A-Za-z0-9_-]+)$/.exec(hash);
  if (!m) return null;
  try {
    return sanitize(JSON.parse(fromBase64Url(m[1]!)));
  } catch {
    return null;
  }
}

/** JSON the core accepts, as NUL-terminated UTF-8 for the worklet. */
export function specBytes(spec: PatternSpec): Uint8Array {
  const json = JSON.stringify(spec);
  const body = new TextEncoder().encode(json);
  const out = new Uint8Array(body.length + 1);
  out.set(body);
  return out;
}
