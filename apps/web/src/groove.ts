// NI Maschine-inspired Groove, Humanize & Variation generator + Per-Voice /
// Group FX definitions for player5.

import { cloneSpec, normalizeSteps, sanitize, STEP_COUNT, type PatternSpec, type StepChar } from "./spec";
import { VOICES, type VoiceId } from "./voices";

export type GrooveTemplateId =
  | "tight"
  | "mpc62"
  | "maschine"
  | "drummer"
  | "jackin"
  | "broken";

export interface GrooveTemplate {
  id: GrooveTemplateId;
  name: string;
  shuffle: number;
  humanize: number;
  accent: number;
  flam: number;
  blurb: string;
}

export const GROOVE_TEMPLATES: readonly GrooveTemplate[] = [
  {
    id: "tight",
    name: "Tight Studio · 0% Drift",
    shuffle: 0,
    humanize: 0,
    accent: 0.55,
    flam: 0.32,
    blurb: "Hardware-locked 16th grid with zero micro-timing drift",
  },
  {
    id: "mpc62",
    name: "MPC 62% Swing Pocket",
    shuffle: 0.48,
    humanize: 0.28,
    accent: 0.68,
    flam: 0.36,
    blurb: "Classic 62% 16th-note swing with warm velocity pocket",
  },
  {
    id: "maschine",
    name: "Maschine Laid-Back Feel",
    shuffle: 0.34,
    humanize: 0.46,
    accent: 0.64,
    flam: 0.42,
    blurb: "Late snare pocket, humanized ghost notes, and organic hat drift",
  },
  {
    id: "drummer",
    name: "Live Drummer Pocket",
    shuffle: 0.26,
    humanize: 0.65,
    accent: 0.74,
    flam: 0.48,
    blurb: "Expressive wrist dynamics, loose hats, and natural flam drags",
  },
  {
    id: "jackin",
    name: "House Jackin' Push",
    shuffle: 0.44,
    humanize: 0.32,
    accent: 0.78,
    flam: 0.35,
    blurb: "Pushed off-beat hi-hats and punchy Chicago/French house swing",
  },
  {
    id: "broken",
    name: "Broken Techno Micro-Groove",
    shuffle: 0.18,
    humanize: 0.52,
    accent: 0.7,
    flam: 0.4,
    blurb: "Subtle micro-timing tension and analog timbre drift per step",
  },
];

export type VoiceFxKind = "echo" | "reverb" | "filter";

export interface VoiceFxSend {
  echo: boolean;
  reverb: boolean;
  filter: boolean;
}

export type VoiceFxMap = Record<VoiceId, VoiceFxSend>;

export type FxGroupId = "snare_clap" | "hats" | "toms_perc" | "toms" | "low_end";

export interface FxGroupInfo {
  id: FxGroupId;
  label: string;
  short: string;
  shortcut: string;
  voices: readonly VoiceId[];
}

export const FX_GROUPS: readonly FxGroupInfo[] = [
  {
    id: "snare_clap",
    label: "Snare + Clap + Rim",
    short: "SD·CP·RS",
    shortcut: "Shift+Q",
    voices: ["snare", "clap", "rim"],
  },
  {
    id: "hats",
    label: "Hi-Hats (CH + OH)",
    short: "CH·OH",
    shortcut: "Shift+W",
    voices: ["closed_hat", "open_hat"],
  },
  {
    id: "toms_perc",
    label: "Toms + Cowbell",
    short: "LT·MT·HT·CB",
    shortcut: "Shift+E",
    voices: ["low_tom", "mid_tom", "high_tom", "cowbell"],
  },
  {
    id: "toms",
    label: "Toms + Cowbell",
    short: "LT·MT·HT·CB",
    shortcut: "Shift+E",
    voices: ["low_tom", "mid_tom", "high_tom", "cowbell"],
  },
  {
    id: "low_end",
    label: "Kick + Low Tom",
    short: "BD·LT",
    shortcut: "Shift+D",
    voices: ["kick", "low_tom"],
  },
];

export function defaultVoiceFxMap(): VoiceFxMap {
  return {
    kick: { echo: false, reverb: false, filter: false },
    snare: { echo: false, reverb: false, filter: false },
    low_tom: { echo: false, reverb: false, filter: false },
    mid_tom: { echo: false, reverb: false, filter: false },
    high_tom: { echo: false, reverb: false, filter: false },
    rim: { echo: false, reverb: false, filter: false },
    clap: { echo: false, reverb: false, filter: false },
    closed_hat: { echo: false, reverb: false, filter: false },
    open_hat: { echo: false, reverb: false, filter: false },
    cowbell: { echo: false, reverb: false, filter: false },
  };
}

export function cloneVoiceFxMap(map: VoiceFxMap): VoiceFxMap {
  const out = defaultVoiceFxMap();
  for (const v of VOICES) {
    out[v.id] = { ...map[v.id] };
  }
  return out;
}

export function clearAllVoiceFx(map: VoiceFxMap): void {
  for (const v of VOICES) {
    map[v.id].echo = false;
    map[v.id].reverb = false;
    map[v.id].filter = false;
  }
}

export function hasAnyVoiceFx(map: VoiceFxMap): boolean {
  return VOICES.some((v) => map[v.id].echo || map[v.id].reverb || map[v.id].filter);
}

export function isGroupEchoActive(map: VoiceFxMap, groupId: FxGroupId): boolean {
  const grp = FX_GROUPS.find((g) => g.id === groupId);
  if (!grp) return false;
  return grp.voices.every((vid) => map[vid].echo);
}

export function toggleGroupFx(
  map: VoiceFxMap,
  groupId: FxGroupId,
  fx: VoiceFxKind = "echo",
): boolean {
  const grp = FX_GROUPS.find((g) => g.id === groupId);
  if (!grp) return false;
  const allOn = grp.voices.every((vid) => map[vid][fx]);
  const nextVal = !allOn;
  for (const vid of grp.voices) {
    map[vid][fx] = nextVal;
  }
  return nextVal;
}

let seedState = 0x9e3779b9;
function nextRand(): number {
  seedState ^= seedState << 13;
  seedState ^= seedState >>> 17;
  seedState ^= seedState << 5;
  return (seedState >>> 0) / 0xffffffff;
}

const jitterParam = (v: number, span: number) =>
  Math.max(0.05, Math.min(0.98, Math.round((v + (nextRand() * 2 - 1) * span) * 100) / 100));

/**
 * NI Maschine-style Humanize: injects natural velocity dynamics (accents on
 * groove beats, normal hits on off-beats, occasional ghost flams on turnarounds)
 * and subtle organic voice parameter variation without breaking the rhythm.
 */
export function humanizePattern(
  spec: PatternSpec,
  scope: VoiceId | "all" = "all",
  amount = 0.45,
): PatternSpec {
  const next = cloneSpec(spec);
  const intensity = Math.max(0.1, Math.min(1, amount));
  const targets = scope === "all" ? VOICES.map((v) => v.id) : [scope];

  for (const vid of targets) {
    const v = next.voices[vid];
    const chars = [...normalizeSteps(v.steps)] as StepChar[];
    for (let i = 0; i < STEP_COUNT; i++) {
      const c = chars[i]!;
      if (c === "-") {
        // Occasionally add a soft ghost hit on turnaround steps (14, 15) for hats/snare/rim
        if (
          (vid === "closed_hat" || vid === "rim" || vid === "snare") &&
          (i === 14 || i === 15 || i === 7) &&
          nextRand() < intensity * 0.22
        ) {
          chars[i] = "x";
        }
        continue;
      }
      const isDownbeat = i % 4 === 0;
      const isOffbeatEighth = i % 4 === 2;
      const r = nextRand();
      if (vid === "kick") {
        // Keep kick downbeats strong, humanize syncopated pickups
        if (isDownbeat) chars[i] = r < 0.8 ? "X" : "x";
        else chars[i] = r < 0.25 ? "X" : "x";
      } else if (vid === "snare" || vid === "clap") {
        if (i === 4 || i === 12) {
          chars[i] = r < intensity * 0.28 ? "F" : "X";
        } else if (r < intensity * 0.35) {
          chars[i] = c === "x" ? "f" : "x";
        }
      } else if (vid === "closed_hat" || vid === "open_hat") {
        if (isOffbeatEighth) chars[i] = r < 0.75 ? "X" : "x";
        else if (i % 2 === 1 && r < intensity * 0.2) chars[i] = "f";
        else chars[i] = r < 0.3 ? "X" : "x";
      } else {
        chars[i] = r < 0.45 ? "X" : r < 0.62 ? "f" : "x";
      }
    }
    v.steps = chars.join("");
    const span = 0.05 * intensity;
    v.tune = jitterParam(v.tune, span);
    v.decay = jitterParam(v.decay, span);
    v.tone = jitterParam(v.tone, span);
    v.snappy = jitterParam(v.snappy, span);
  }

  if (next.shuffle < 0.08) {
    next.shuffle = Math.round((0.14 + intensity * 0.18) * 100) / 100;
  }
  return sanitize(next);
}

/**
 * NI Maschine-style Variation / Mutate: generates a musical rhythmic variation
 * that respects house/techno/disco groove structure (anchors kick/backbeat
 * while evolving syncopations, hats, toms, and percussion).
 */
export function mutatePattern(
  spec: PatternSpec,
  scope: VoiceId | "all" = "all",
  spice = 0.45,
): PatternSpec {
  const next = cloneSpec(spec);
  const density = Math.max(0.15, Math.min(1, spice));
  const targets = scope === "all" ? VOICES.map((v) => v.id) : [scope];

  for (const vid of targets) {
    const v = next.voices[vid];
    const chars = [...normalizeSteps(v.steps)] as StepChar[];

    switch (vid) {
      case "kick":
        // Keep 4-on-the-floor or main downbeats intact, vary ghost pickups (steps 4, 8, 12, 15, 16)
        for (const idx of [3, 6, 7, 11, 14, 15]) {
          if (nextRand() < density * 0.45) {
            chars[idx] = chars[idx] === "-" ? "x" : "-";
          }
        }
        if (chars[0] === "-") chars[0] = "X";
        break;
      case "snare":
      case "clap":
        // Keep backbeats 5 & 13 (indices 4 & 12), vary ghost hits on 2, 8, 10, 15, 16
        for (const idx of [1, 7, 9, 11, 14, 15]) {
          if (nextRand() < density * 0.42) {
            chars[idx] = chars[idx] === "-" ? (nextRand() < 0.35 ? "f" : "x") : "-";
          }
        }
        break;
      case "closed_hat":
        for (let i = 0; i < STEP_COUNT; i++) {
          if (i % 2 === 0) {
            chars[i] = nextRand() < 0.82 ? "x" : "-";
          } else if (nextRand() < density * 0.65) {
            chars[i] = nextRand() < 0.3 ? "X" : nextRand() < 0.2 ? "f" : "x";
          } else {
            chars[i] = "-";
          }
        }
        break;
      case "open_hat":
        for (let i = 0; i < STEP_COUNT; i++) {
          if (i % 4 === 2) {
            chars[i] = nextRand() < 0.85 ? "X" : "x";
          } else if ((i === 15 || i === 7) && nextRand() < density * 0.4) {
            chars[i] = "x";
          } else if (nextRand() < 0.75) {
            chars[i] = "-";
          }
        }
        break;
      case "rim":
      case "cowbell":
        for (const idx of [0, 2, 3, 5, 6, 9, 10, 11, 14, 15]) {
          if (nextRand() < density * 0.32) {
            chars[idx] = chars[idx] === "-" ? (nextRand() < 0.3 ? "X" : "x") : "-";
          }
        }
        break;
      case "low_tom":
      case "mid_tom":
      case "high_tom":
        for (const idx of [2, 3, 6, 7, 10, 11, 13, 14, 15]) {
          if (nextRand() < density * 0.28) {
            chars[idx] = chars[idx] === "-" ? (nextRand() < 0.35 ? "X" : "x") : "-";
          }
        }
        break;
    }
    v.steps = chars.join("");
  }

  return sanitize(next);
}

/**
 * TR-8S / Maschine Auto-Fill: writes an energetic 4-step turnaround fill into
 * steps 13–16 (snare flam roll, tom cascade, clap accent, and open hat lift).
 */
export function applyAutoFill(spec: PatternSpec): PatternSpec {
  const next = cloneSpec(spec);
  const setTail = (vid: VoiceId, tail4: string) => {
    const cur = normalizeSteps(next.voices[vid].steps);
    next.voices[vid].steps = cur.slice(0, 12) + tail4;
  };
  setTail("snare", "xXfF");
  setTail("clap", "X-XX");
  setTail("high_tom", "xx--");
  setTail("mid_tom", "-xx-");
  setTail("low_tom", "--XX");
  setTail("open_hat", "---X");
  return sanitize(next);
}
