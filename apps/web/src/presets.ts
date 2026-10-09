// Built-in patterns. All original: grooves written for player5, with our
// own names.

import { sanitize, type PatternSpec } from "./spec";

export interface Preset {
  id: string;
  name: string;
  /** One line for the menu tooltip. */
  blurb: string;
  spec: () => PatternSpec;
}

const preset = (id: string, name: string, blurb: string, raw: object): Preset => ({
  id,
  name,
  blurb,
  spec: () => sanitize(raw),
});

export const PRESETS: readonly Preset[] = [
  preset("basement-four", "Basement Four", "House: four on the floor, off-beat open hats", {
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
  preset("night-shift", "Night Shift", "Techno: driving kick, rolling low tom, sixteenth hats", {
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
  preset("circuit-break", "Circuit Break", "Electro: broken kick, hard snare, cowbell stabs", {
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
  preset("late-swing", "Late Swing", "Shuffled groove with flammed snares", {
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
  preset("bare-kick", "Bare Kick", "Just the bass drum, for checking levels", {
    bpm: 124,
    shuffle: 0,
    accent: 0.5,
    flam: 0.5,
    voices: { kick: { steps: "X---x---X---x---" } },
  }),
];

export const DEFAULT_PRESET = PRESETS[0]!;
