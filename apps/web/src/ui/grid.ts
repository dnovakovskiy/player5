// The 10 × 16 step grid: row labels (select voice), mute, steps, playhead.
// Keyboard: steps use a roving tabindex; arrows move, Home/End jump,
// Enter toggles (Space is the global play/stop key).

import { describeStep, normalizeSteps, STEP_COUNT, type PatternSpec, type StepChar } from "../spec";
import { VOICES, type VoiceId } from "../voices";

export interface GridHandlers {
  onStep(voice: VoiceId, step: number): void;
  onSelect(voice: VoiceId): void;
  onMute(voice: VoiceId): void;
}

const STATE: Record<StepChar, string> = {
  "-": "off",
  x: "on",
  X: "accent",
  f: "flam",
  F: "accent-flam",
};

export class Grid {
  private readonly steps: HTMLButtonElement[][] = [];
  private readonly rows: HTMLElement[] = [];
  private readonly selectButtons: HTMLButtonElement[] = [];
  private readonly muteButtons: HTMLButtonElement[] = [];
  private focusRow = 0;
  private focusCol = 0;
  private head = -1;

  constructor(
    private readonly root: HTMLElement,
    private readonly handlers: GridHandlers,
  ) {
    VOICES.forEach((voice, r) => {
      const row = document.createElement("div");
      row.className = "row";
      row.dataset.voice = voice.id;
      row.setAttribute("role", "group");
      row.setAttribute("aria-label", voice.name);

      const head = document.createElement("div");
      head.className = "row-head";
      const sel = document.createElement("button");
      sel.type = "button";
      sel.className = "voice-btn";
      sel.textContent = voice.short;
      sel.dataset.select = voice.id;
      sel.title = `${voice.name}: show its controls`;
      sel.setAttribute("aria-label", `${voice.name} controls`);
      sel.addEventListener("click", () => handlers.onSelect(voice.id));
      const mute = document.createElement("button");
      mute.type = "button";
      mute.className = "mute";
      mute.textContent = "M";
      mute.dataset.mute = voice.id;
      mute.title = `Mute ${voice.name}`;
      mute.setAttribute("aria-label", `Mute ${voice.name}`);
      mute.addEventListener("click", () => handlers.onMute(voice.id));
      head.append(sel, mute);
      row.append(head);
      this.selectButtons.push(sel);
      this.muteButtons.push(mute);

      const buttons: HTMLButtonElement[] = [];
      for (let i = 0; i < STEP_COUNT; i++) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = `step q${i >> 2}${i % 4 === 0 ? " beat" : ""}`;
        b.dataset.voice = voice.id;
        b.dataset.step = String(i + 1);
        b.tabIndex = r === 0 && i === 0 ? 0 : -1;
        b.setAttribute("aria-label", `${voice.name} step ${i + 1}`);
        b.addEventListener("click", () => {
          this.setFocus(r, i, false);
          handlers.onStep(voice.id, i);
        });
        b.addEventListener("keydown", (e) => this.onKey(e, r, i));
        buttons.push(b);
        row.append(b);
      }
      this.steps.push(buttons);
      this.rows.push(row);
      root.append(row);
    });
  }

  render(spec: PatternSpec, selected: VoiceId): void {
    VOICES.forEach((voice, r) => {
      const v = spec.voices[voice.id];
      const steps = normalizeSteps(v.steps);
      const row = this.rows[r]!;
      row.classList.toggle("muted", v.mute);
      row.classList.toggle("selected", voice.id === selected);
      this.selectButtons[r]!.setAttribute("aria-pressed", String(voice.id === selected));
      this.muteButtons[r]!.setAttribute("aria-pressed", String(v.mute));
      this.steps[r]!.forEach((b, i) => {
        const c = (steps[i] ?? "-") as StepChar;
        const state = STATE[c];
        if (b.dataset.state !== state) {
          b.dataset.state = state;
          b.setAttribute("aria-pressed", String(c !== "-"));
          b.title = `${voice.name} step ${i + 1}: ${describeStep(c)}`;
        }
      });
    });
  }

  /** Lights the playhead column (`-1` hides it). */
  setPlayhead(step: number): void {
    if (step === this.head) return;
    if (this.head >= 0) for (const row of this.steps) row[this.head]?.classList.remove("head");
    this.head = step;
    if (step >= 0) for (const row of this.steps) row[step]?.classList.add("head");
  }

  private setFocus(r: number, c: number, focus: boolean): void {
    this.steps[this.focusRow]![this.focusCol]!.tabIndex = -1;
    this.focusRow = r;
    this.focusCol = c;
    const b = this.steps[r]![c]!;
    b.tabIndex = 0;
    if (focus) b.focus();
  }

  private onKey(e: KeyboardEvent, r: number, c: number): void {
    const rows = this.steps.length;
    let nr = r;
    let nc = c;
    switch (e.key) {
      case "ArrowLeft":
        nc = (c + STEP_COUNT - 1) % STEP_COUNT;
        break;
      case "ArrowRight":
        nc = (c + 1) % STEP_COUNT;
        break;
      case "ArrowUp":
        nr = (r + rows - 1) % rows;
        break;
      case "ArrowDown":
        nr = (r + 1) % rows;
        break;
      case "Home":
        nc = 0;
        break;
      case "End":
        nc = STEP_COUNT - 1;
        break;
      default:
        return;
    }
    e.preventDefault();
    this.setFocus(nr, nc, true);
  }
}
