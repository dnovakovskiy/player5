// The ten voices of the core (core/engine/src/spec.rs) and the controls
// each one uses.

export type VoiceId =
  | "kick"
  | "snare"
  | "low_tom"
  | "mid_tom"
  | "high_tom"
  | "rim"
  | "clap"
  | "closed_hat"
  | "open_hat"
  | "cowbell";

export type ControlId = "tune" | "decay" | "tone" | "snappy" | "level";

export interface VoiceInfo {
  id: VoiceId;
  /** Two-letter row label. */
  short: string;
  /** Full name for tooltips and screen readers. */
  name: string;
  /** Controls this voice responds to, in panel order. */
  controls: readonly ControlId[];
}

export const VOICES: readonly VoiceInfo[] = [
  { id: "kick", short: "BD", name: "Bass drum", controls: ["tune", "decay", "level"] },
  { id: "snare", short: "SD", name: "Snare drum", controls: ["tune", "tone", "snappy", "decay", "level"] },
  { id: "low_tom", short: "LT", name: "Low tom", controls: ["tune", "decay", "level"] },
  { id: "mid_tom", short: "MT", name: "Mid tom", controls: ["tune", "decay", "level"] },
  { id: "high_tom", short: "HT", name: "High tom", controls: ["tune", "decay", "level"] },
  { id: "rim", short: "RS", name: "Rimshot", controls: ["tune", "tone", "decay", "level"] },
  { id: "clap", short: "CP", name: "Hand clap", controls: ["tone", "decay", "level"] },
  { id: "closed_hat", short: "CH", name: "Closed hi-hat", controls: ["tone", "decay", "level"] },
  { id: "open_hat", short: "OH", name: "Open hi-hat", controls: ["tone", "decay", "level"] },
  { id: "cowbell", short: "CB", name: "Cowbell", controls: ["tune", "tone", "decay", "level"] },
];

export const VOICE_IDS: readonly VoiceId[] = VOICES.map((v) => v.id);

export function voiceInfo(id: VoiceId): VoiceInfo {
  return VOICES.find((v) => v.id === id)!;
}

/** Defaults of the core's pattern format (serde defaults in spec.rs). */
export const CONTROL_DEFAULTS: Readonly<Record<ControlId, number>> = {
  tune: 0.5,
  decay: 0.5,
  tone: 0.5,
  snappy: 0.5,
  level: 1,
};

export const CONTROL_IDS: readonly ControlId[] = ["tune", "decay", "tone", "snappy", "level"];

export const CONTROL_LABELS: Readonly<Record<ControlId, string>> = {
  tune: "Tune",
  decay: "Decay",
  tone: "Tone",
  snappy: "Snappy",
  level: "Level",
};
