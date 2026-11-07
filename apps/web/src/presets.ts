// Built-in patterns and analog drum machine kit voicings.
// All grooves and voicings are synthesized natively by player5.

import { sanitize, type PatternSpec, type VoiceSpec } from "./spec";
import { VOICES, type VoiceId } from "./voices";

export type PresetCategory = "Core Booth" | "Nu-Disco" | "House" | "Techno";

export interface KitPreset {
  id: string;
  name: string;
  blurb: string;
  accent?: number;
  flam?: number;
  voices: Record<VoiceId, Partial<Omit<VoiceSpec, "steps" | "mute">>>;
}

export interface Preset {
  id: string;
  name: string;
  category: PresetCategory;
  kitId?: string;
  /** One line for the menu tooltip. */
  blurb: string;
  spec: () => PatternSpec;
}

/**
 * Five classic analog drum machine circuit voicings tuned for the ten
 * synthesized voices (bridged-T kick, dual-shell snare, 4-burst flutter clap,
 * 6-oscillator metallic square-wave hats, dual-square cowbell, dual-ping rim,
 * and damped swept toms).
 */
export const KITS: readonly KitPreset[] = [
  {
    id: "sub-boom",
    name: "Analog Sub Boom",
    blurb: "Deep booming bridged-T sub kick, warm low snare, silky 6-oscillator metallic hats & dual-square cowbell",
    accent: 0.65,
    flam: 0.45,
    voices: {
      kick: { tune: 0.22, decay: 0.86, tone: 0.42, snappy: 0.5, level: 0.95 },
      snare: { tune: 0.38, decay: 0.35, tone: 0.48, snappy: 0.65, level: 0.85 },
      low_tom: { tune: 0.32, decay: 0.68, tone: 0.4, snappy: 0.5, level: 0.78 },
      mid_tom: { tune: 0.38, decay: 0.64, tone: 0.44, snappy: 0.5, level: 0.76 },
      high_tom: { tune: 0.44, decay: 0.6, tone: 0.48, snappy: 0.5, level: 0.74 },
      rim: { tune: 0.48, decay: 0.4, tone: 0.38, snappy: 0.5, level: 0.72 },
      clap: { tune: 0.5, decay: 0.72, tone: 0.52, snappy: 0.5, level: 0.84 },
      closed_hat: { tune: 0.46, decay: 0.32, tone: 0.55, snappy: 0.5, level: 0.72 },
      open_hat: { tune: 0.46, decay: 0.68, tone: 0.55, snappy: 0.5, level: 0.76 },
      cowbell: { tune: 0.5, decay: 0.58, tone: 0.55, snappy: 0.5, level: 0.72 },
    },
  },
  {
    id: "club-punch",
    name: "Club Punch Machine",
    blurb: "High-attack punchy club kick, crisp snappy snare, tight clap & bright sizzle hats",
    accent: 0.7,
    flam: 0.42,
    voices: {
      kick: { tune: 0.64, decay: 0.46, tone: 0.78, snappy: 0.5, level: 0.95 },
      snare: { tune: 0.58, decay: 0.48, tone: 0.74, snappy: 0.88, level: 0.86 },
      low_tom: { tune: 0.54, decay: 0.5, tone: 0.7, snappy: 0.5, level: 0.78 },
      mid_tom: { tune: 0.58, decay: 0.48, tone: 0.72, snappy: 0.5, level: 0.76 },
      high_tom: { tune: 0.62, decay: 0.45, tone: 0.75, snappy: 0.5, level: 0.74 },
      rim: { tune: 0.62, decay: 0.48, tone: 0.72, snappy: 0.5, level: 0.72 },
      clap: { tune: 0.5, decay: 0.48, tone: 0.68, snappy: 0.5, level: 0.84 },
      closed_hat: { tune: 0.58, decay: 0.42, tone: 0.78, snappy: 0.5, level: 0.74 },
      open_hat: { tune: 0.58, decay: 0.76, tone: 0.78, snappy: 0.5, level: 0.8 },
      cowbell: { tune: 0.54, decay: 0.45, tone: 0.65, snappy: 0.5, level: 0.68 },
    },
  },
  {
    id: "studio-disco",
    name: "Dry Studio Disco",
    blurb: "Tight punchy disco kick, wide hand-clap, chick closed hats, funky cowbell & woody rimshot",
    accent: 0.58,
    flam: 0.36,
    voices: {
      kick: { tune: 0.52, decay: 0.34, tone: 0.64, snappy: 0.5, level: 0.92 },
      snare: { tune: 0.46, decay: 0.32, tone: 0.58, snappy: 0.74, level: 0.84 },
      low_tom: { tune: 0.48, decay: 0.42, tone: 0.62, snappy: 0.5, level: 0.76 },
      mid_tom: { tune: 0.52, decay: 0.4, tone: 0.64, snappy: 0.5, level: 0.75 },
      high_tom: { tune: 0.56, decay: 0.38, tone: 0.68, snappy: 0.5, level: 0.74 },
      rim: { tune: 0.54, decay: 0.35, tone: 0.48, snappy: 0.5, level: 0.76 },
      clap: { tune: 0.5, decay: 0.62, tone: 0.64, snappy: 0.5, level: 0.86 },
      closed_hat: { tune: 0.52, decay: 0.28, tone: 0.68, snappy: 0.5, level: 0.72 },
      open_hat: { tune: 0.52, decay: 0.58, tone: 0.72, snappy: 0.5, level: 0.78 },
      cowbell: { tune: 0.52, decay: 0.38, tone: 0.62, snappy: 0.5, level: 0.75 },
    },
  },
  {
    id: "silver-electro",
    name: "Silver Matrix",
    blurb: "Fast-decay analog electro kit: snappy mid kick, papery wire snare & crisp metallic hats",
    accent: 0.66,
    flam: 0.4,
    voices: {
      kick: { tune: 0.56, decay: 0.3, tone: 0.68, snappy: 0.5, level: 0.92 },
      snare: { tune: 0.66, decay: 0.28, tone: 0.8, snappy: 0.92, level: 0.84 },
      low_tom: { tune: 0.6, decay: 0.38, tone: 0.74, snappy: 0.5, level: 0.76 },
      mid_tom: { tune: 0.64, decay: 0.36, tone: 0.76, snappy: 0.5, level: 0.75 },
      high_tom: { tune: 0.7, decay: 0.34, tone: 0.78, snappy: 0.5, level: 0.74 },
      rim: { tune: 0.66, decay: 0.32, tone: 0.78, snappy: 0.5, level: 0.74 },
      clap: { tune: 0.5, decay: 0.42, tone: 0.75, snappy: 0.5, level: 0.8 },
      closed_hat: { tune: 0.72, decay: 0.24, tone: 0.84, snappy: 0.5, level: 0.72 },
      open_hat: { tune: 0.72, decay: 0.52, tone: 0.84, snappy: 0.5, level: 0.74 },
      cowbell: { tune: 0.6, decay: 0.42, tone: 0.72, snappy: 0.5, level: 0.72 },
    },
  },
  {
    id: "warehouse-raw",
    name: "Raw Warehouse",
    blurb: "Heavy industrial & peak-time techno kit: driven sub kick, sub-bass rumble tom & cutting hats",
    accent: 0.78,
    flam: 0.5,
    voices: {
      kick: { tune: 0.34, decay: 0.78, tone: 0.86, snappy: 0.5, level: 0.98 },
      snare: { tune: 0.34, decay: 0.62, tone: 0.64, snappy: 0.9, level: 0.86 },
      low_tom: { tune: 0.14, decay: 0.74, tone: 0.32, snappy: 0.5, level: 0.82 },
      mid_tom: { tune: 0.3, decay: 0.66, tone: 0.5, snappy: 0.5, level: 0.78 },
      high_tom: { tune: 0.42, decay: 0.58, tone: 0.62, snappy: 0.5, level: 0.74 },
      rim: { tune: 0.38, decay: 0.55, tone: 0.64, snappy: 0.5, level: 0.7 },
      clap: { tune: 0.5, decay: 0.84, tone: 0.36, snappy: 0.5, level: 0.84 },
      closed_hat: { tune: 0.42, decay: 0.36, tone: 0.88, snappy: 0.5, level: 0.74 },
      open_hat: { tune: 0.42, decay: 0.82, tone: 0.88, snappy: 0.5, level: 0.8 },
      cowbell: { tune: 0.38, decay: 0.62, tone: 0.48, snappy: 0.5, level: 0.68 },
    },
  },
];

/**
 * Applies a drum kit's analog synthesis parameters to a pattern spec while
 * preserving every voice's step sequence and mute state.
 */
export function applyKitToSpec(current: PatternSpec, kit: KitPreset): PatternSpec {
  const next: PatternSpec = JSON.parse(JSON.stringify(current));
  if (kit.accent !== undefined) next.accent = kit.accent;
  if (kit.flam !== undefined) next.flam = kit.flam;
  for (const v of VOICES) {
    const patch = kit.voices[v.id];
    if (!patch) continue;
    const target = next.voices[v.id];
    if (patch.tune !== undefined) target.tune = patch.tune;
    if (patch.decay !== undefined) target.decay = patch.decay;
    if (patch.tone !== undefined) target.tone = patch.tone;
    if (patch.snappy !== undefined) target.snappy = patch.snappy;
    if (patch.level !== undefined) target.level = patch.level;
  }
  return sanitize(next);
}

const preset = (
  id: string,
  name: string,
  category: PresetCategory,
  blurb: string,
  raw: object,
  kitId?: string,
): Preset => ({
  id,
  name,
  category,
  kitId,
  blurb,
  spec: () => {
    const base = sanitize(raw);
    if (!kitId) return base;
    const kit = KITS.find((k) => k.id === kitId);
    if (!kit) return base;
    // Apply kit defaults first, then let explicit voice tweaks in `raw` win.
    const withKit = applyKitToSpec(base, kit);
    const rawVoices = (raw as { voices?: Record<string, Record<string, unknown>> }).voices ?? {};
    for (const [vid, overrides] of Object.entries(rawVoices)) {
      const target = withKit.voices[vid as VoiceId];
      if (!target || !overrides) continue;
      for (const k of ["tune", "decay", "tone", "snappy", "level"] as const) {
        if (typeof overrides[k] === "number") target[k] = overrides[k] as number;
      }
    }
    return sanitize(withKit);
  },
});

export const PRESETS: readonly Preset[] = [
  // ---- Core Booth (Original 5 presets) ----
  preset("basement-four", "Basement Four", "Core Booth", "House: four on the floor, off-beat open hats", {
    bpm: 124,
    shuffle: 0.12,
    accent: 0.6,
    flam: 0.4,
    voices: {
      kick: { steps: "X---x---X---x---" },
      clap: { steps: "----x-------x---" },
      closed_hat: { steps: "x---x---x---x---", decay: 0.35, level: 0.7 },
      open_hat: { steps: "--x---x---x---x-", level: 0.8 },
      rim: { steps: "-------x--x-----", level: 0.6 },
    },
  }),
  preset("night-shift", "Night Shift", "Core Booth", "Techno: driving kick, rolling low tom, sixteenth hats", {
    bpm: 132,
    shuffle: 0,
    accent: 0.7,
    flam: 0.5,
    voices: {
      kick: { steps: "X---X---X---X---", decay: 0.6 },
      low_tom: { steps: "--x---x---x--x-x", tune: 0.25, decay: 0.7, level: 0.7 },
      clap: { steps: "----x-------x---", tone: 0.65, level: 0.8 },
      closed_hat: { steps: "xxXxxxXxxxXxxxXx", decay: 0.25, level: 0.6 },
      open_hat: { steps: "--------------x-", level: 0.6 },
      rim: { steps: "---x--x----x--x-", tone: 0.7, level: 0.5 },
    },
  }),
  preset("circuit-break", "Circuit Break", "Core Booth", "Electro: broken kick, hard snare, cowbell stabs", {
    bpm: 118,
    shuffle: 0,
    accent: 0.65,
    flam: 0.45,
    voices: {
      kick: { steps: "X------x--X-----", tune: 0.4, decay: 0.65 },
      snare: { steps: "----X-------X---", snappy: 0.7 },
      clap: { steps: "----x-------x--x", level: 0.6 },
      closed_hat: { steps: "x-x-x-x-x-x-xXx-", decay: 0.3, level: 0.7 },
      cowbell: { steps: "------x-----x---", level: 0.6 },
      high_tom: { steps: "--------------xx", level: 0.6 },
    },
  }),
  preset("late-swing", "Late Swing", "Core Booth", "Shuffled groove with flammed snares", {
    bpm: 112,
    shuffle: 0.6,
    accent: 0.6,
    flam: 0.55,
    voices: {
      kick: { steps: "X-----x-x-----x-" },
      snare: { steps: "----F--x----F---", tone: 0.4, snappy: 0.55 },
      closed_hat: { steps: "x-xxx-xxx-xxx-xx", decay: 0.3, level: 0.65 },
      open_hat: { steps: "------x-------x-", level: 0.6 },
      rim: { steps: "-x-----x-x------", level: 0.5 },
      mid_tom: { steps: "-----------x--f-", level: 0.6 },
    },
  }),
  preset("bare-kick", "Bare Kick", "Core Booth", "Just the bass drum, for checking levels", {
    bpm: 124,
    shuffle: 0,
    accent: 0.5,
    flam: 0.5,
    voices: { kick: { steps: "X---x---X---x---" } },
  }),

  // ---- Nu-Disco ----
  preset(
    "parisian-filter",
    "Parisian Filter",
    "Nu-Disco",
    "Nu-Disco: French-touch four-on-the-floor, flammed hand-clap, disco cowbell & open hat lift",
    {
      bpm: 120,
      shuffle: 0.16,
      accent: 0.62,
      flam: 0.36,
      voices: {
        kick: { steps: "X---X---X---X-x-" },
        snare: { steps: "----x-------x---" },
        clap: { steps: "----F-------F---" },
        closed_hat: { steps: "x-x-x-x-x-x-x-x-" },
        open_hat: { steps: "--X---X---X---X-" },
        rim: { steps: "---x--x----x----" },
        cowbell: { steps: "------x-------x-" },
      },
    },
    "studio-disco",
  ),
  preset(
    "italo-cosmic",
    "Italo Cosmic",
    "Nu-Disco",
    "Nu-Disco: propulsive octave-style tom arpeggio, crisp backbeat snare & chugging 16th hats",
    {
      bpm: 122,
      shuffle: 0,
      accent: 0.68,
      flam: 0.4,
      voices: {
        kick: { steps: "X---x---X---x---" },
        snare: { steps: "----X-------X---" },
        clap: { steps: "----x-------x-x-" },
        low_tom: { steps: "--x---x---x-----", tune: 0.28 },
        mid_tom: { steps: "----x-------x---", tune: 0.54 },
        high_tom: { steps: "-------x------xx", tune: 0.78 },
        closed_hat: { steps: "xx-xxx-xxx-xxx-x" },
        open_hat: { steps: "--X---X---X---X-" },
      },
    },
    "silver-electro",
  ),
  preset(
    "boogie-strut",
    "Boogie Strut",
    "Nu-Disco",
    "Nu-Disco: swung electro-boogie pocket with sub kick, flammed clap & funky cowbell",
    {
      bpm: 116,
      shuffle: 0.38,
      accent: 0.65,
      flam: 0.45,
      voices: {
        kick: { steps: "X-----x-X---x---" },
        snare: { steps: "----X--x----X--x" },
        clap: { steps: "----F-------X---" },
        closed_hat: { steps: "x-xxx-xxx-xxx-xx" },
        open_hat: { steps: "--x-------x---x-" },
        rim: { steps: "-x-----x---x----" },
        cowbell: { steps: "--x---x---x--x--" },
        mid_tom: { steps: "--------------xx" },
      },
    },
    "sub-boom",
  ),
  preset(
    "balearic-sunset",
    "Balearic Sunset",
    "Nu-Disco",
    "Nu-Disco: warm sunset chugger with clave rimshot, conga-tuned toms & airy hats",
    {
      bpm: 118,
      shuffle: 0.22,
      accent: 0.58,
      flam: 0.42,
      voices: {
        kick: { steps: "X---x---X---x---" },
        clap: { steps: "----x-------x---" },
        low_tom: { steps: "---x-----x------", tune: 0.36, decay: 0.55 },
        mid_tom: { steps: "------x-----x--x", tune: 0.58, decay: 0.5 },
        high_tom: { steps: "-----------x--x-", tune: 0.74, decay: 0.45 },
        rim: { steps: "X--x--X---x-x---" },
        closed_hat: { steps: "x-x-x-x-x-x-x-x-" },
        open_hat: { steps: "------X-------X-" },
      },
    },
    "studio-disco",
  ),

  // ---- House ----
  preset(
    "warehouse-jack",
    "Warehouse Jack",
    "House",
    "House: classic Chicago jackin' groove with booming sub-kick, swung hats & rim/cowbell stabs",
    {
      bpm: 126,
      shuffle: 0.34,
      accent: 0.72,
      flam: 0.45,
      voices: {
        kick: { steps: "X---x---X---x-x-" },
        snare: { steps: "----X--x----X--x" },
        clap: { steps: "----X-------F---" },
        closed_hat: { steps: "x-xxx-xxx-xxx-xx" },
        open_hat: { steps: "--X---X---X---X-" },
        rim: { steps: "---x--x----x----" },
        cowbell: { steps: "------x-------x-" },
      },
    },
    "sub-boom",
  ),
  preset(
    "roule-loop",
    "Roule Loop",
    "House",
    "House: high-impact filtered club loop with punchy kick, double-flam claps & upbeat open hats",
    {
      bpm: 125,
      shuffle: 0.18,
      accent: 0.75,
      flam: 0.38,
      voices: {
        kick: { steps: "X---X---X---X---" },
        snare: { steps: "----x-------x--x" },
        clap: { steps: "----F-------F---" },
        closed_hat: { steps: "x-x-x-x-x-x-x-x-" },
        open_hat: { steps: "--X---X---X---X-" },
        low_tom: { steps: "-----------x----" },
        mid_tom: { steps: "--------------xx" },
      },
    },
    "club-punch",
  ),
  preset(
    "deep-afterhours",
    "Deep Afterhours",
    "House",
    "House: late-night deep roller with warm sub-bass tom groove, dubbed rimshots & shuffled hats",
    {
      bpm: 123,
      shuffle: 0.42,
      accent: 0.6,
      flam: 0.48,
      voices: {
        kick: { steps: "X---x---X---x---" },
        clap: { steps: "----x-------x---", tone: 0.42 },
        low_tom: { steps: "--x----x--x---x-", tune: 0.18, decay: 0.72, level: 0.76 },
        rim: { steps: "---x--x----x--f-" },
        closed_hat: { steps: "x-xxx-xxx-xxx-xx" },
        open_hat: { steps: "--x---x---x---X-" },
      },
    },
    "sub-boom",
  ),
  preset(
    "tech-house-roller",
    "Tech House Roller",
    "House",
    "House: peak-floor tech-house roller with tight kick, syncopated toms, ghost snares & cowbell",
    {
      bpm: 127,
      shuffle: 0.2,
      accent: 0.7,
      flam: 0.4,
      voices: {
        kick: { steps: "X---x---X---x---" },
        snare: { steps: "----x--x-x--x--x" },
        clap: { steps: "----X-------X---" },
        low_tom: { steps: "---x-------x----" },
        mid_tom: { steps: "------x-------x-" },
        high_tom: { steps: "--------------xx" },
        closed_hat: { steps: "xx-xxx-xxx-xxx-x" },
        open_hat: { steps: "--X---X---X---X-" },
        cowbell: { steps: "----------x-----" },
      },
    },
    "club-punch",
  ),

  // ---- Techno ----
  preset(
    "motor-city",
    "Motor City Drive",
    "Techno",
    "Techno: Detroit-style groove with syncopated kick, snappy snare, rimshot & 16th hat ride",
    {
      bpm: 130,
      shuffle: 0.08,
      accent: 0.74,
      flam: 0.44,
      voices: {
        kick: { steps: "X---x--xX---x---" },
        snare: { steps: "----X-------X-x-" },
        clap: { steps: "----x-------X---" },
        rim: { steps: "--x---x---x--x--" },
        closed_hat: { steps: "x-xxx-xxx-xxx-xx" },
        open_hat: { steps: "--X---X---X---X-" },
        high_tom: { steps: "-----------x--f-" },
      },
    },
    "club-punch",
  ),
  preset(
    "sub-basement-rumble",
    "Sub Basement Rumble",
    "Techno",
    "Techno: heavy Berlin peak-time tool with driven kick, 16th low-tom sub rumble & metallic hats",
    {
      bpm: 134,
      shuffle: 0,
      accent: 0.8,
      flam: 0.5,
      voices: {
        kick: { steps: "X---X---X---X---" },
        low_tom: { steps: "-xx--xx--xx--xx-", tune: 0.12, decay: 0.76, tone: 0.25, level: 0.78 },
        clap: { steps: "----x-------x---" },
        rim: { steps: "---x-------x--x-" },
        closed_hat: { steps: "x-x-x-x-x-x-x-x-" },
        open_hat: { steps: "--X---X---X---X-" },
      },
    },
    "warehouse-raw",
  ),
  preset(
    "hypnotic-poly",
    "Hypnotic Polyrhythm",
    "Techno",
    "Techno: 3-against-4 tuned tom & rimshot polymetric sequence over relentless 4/4 kick",
    {
      bpm: 136,
      shuffle: 0,
      accent: 0.75,
      flam: 0.42,
      voices: {
        kick: { steps: "X---X---X---X---" },
        low_tom: { steps: "X--x--X--x--X---", tune: 0.32, decay: 0.52 },
        mid_tom: { steps: "-x--x--x--x--x--", tune: 0.56, decay: 0.46 },
        high_tom: { steps: "--x--x--x--x--x-", tune: 0.76, decay: 0.4 },
        rim: { steps: "---X--x--X--x--x" },
        closed_hat: { steps: "xxXxxxXxxxXxxxXx" },
        open_hat: { steps: "--x---x---x---X-" },
      },
    },
    "silver-electro",
  ),
  preset(
    "hard-groove-tool",
    "Hard Groove Tool",
    "Techno",
    "Techno: high-energy tribal hard-groove loop with flammed snares, interlocking toms & open hats",
    {
      bpm: 138,
      shuffle: 0.2,
      accent: 0.82,
      flam: 0.46,
      voices: {
        kick: { steps: "X---X---X---X-x-" },
        snare: { steps: "--x-F--x--x-F--x" },
        clap: { steps: "----X-------X---" },
        low_tom: { steps: "---x--x----x----" },
        mid_tom: { steps: "------x--x----x-" },
        high_tom: { steps: "-x------------xx" },
        closed_hat: { steps: "x-xxx-xxx-xxx-xx" },
        open_hat: { steps: "--X---X---X---X-" },
        cowbell: { steps: "--x---x---x---x-", level: 0.55 },
      },
    },
    "warehouse-raw",
  ),
];

export const DEFAULT_PRESET = PRESETS[0]!;
