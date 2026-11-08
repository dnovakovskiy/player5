// The 10 × 16 step grid: TR-REC 4-color step scale, row labels (select voice),
// mute, per-voice FX send LEDs (Echo / Reverb / Filter), steps, and playhead.
// Keyboard: steps use a roving tabindex; arrows move, Home/End jump,
// Enter toggles (Space is the global play/stop key).

import type { VoiceFxKind, VoiceFxMap } from "../groove";
import { describeStep, normalizeSteps, STEP_COUNT, type PatternSpec, type StepChar } from "../spec";
import { VOICES, type VoiceId } from "../voices";

export interface GridHandlers {
  onStep(voice: VoiceId, step: number): void;
  onSelect(voice: VoiceId): void;
  onMute(voice: VoiceId): void;
  onToggleFx?(voice: VoiceId, fx: VoiceFxKind): void;
}

const STATE: Record<StepChar, string> = {
  "-": "off",
  x: "on",
  X: "accent",
  f: "flam",
  F: "accent-flam",
};

const VOICE_KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"] as const;
const STEP_KEYS = ["A", "S", "D", "F", "G", "H", "J", "K", "Z", "X", "C", "V", "B", "N", "M", ","] as const;

const FX_DEFS: readonly { kind: VoiceFxKind; short: string; label: string }[] = [
  { kind: "echo", short: "EC", label: "Dub Echo" },
  { kind: "reverb", short: "RV", label: "Space Reverb" },
  { kind: "filter", short: "FL", label: "Acid Filter/Drive" },
];

export class Grid {
  private readonly scaleCells: HTMLElement[] = [];
  private readonly steps: HTMLButtonElement[][] = [];
  private readonly rows: HTMLElement[] = [];
  private readonly selectButtons: HTMLButtonElement[] = [];
  private readonly muteButtons: HTMLButtonElement[] = [];
  private readonly fxButtons: Record<VoiceFxKind, HTMLButtonElement>[] = [];
  private focusRow = 0;
  private focusCol = 0;
  private head = -1;

  constructor(
    private readonly root: HTMLElement,
    private readonly handlers: GridHandlers,
  ) {
    // Classic TR-8S / TR-8O8 16-step color scale bar (Red / Orange / Yellow / Cream)
    const scaleRow = document.createElement("div");
    scaleRow.className = "tr-scale-row";
    scaleRow.setAttribute("aria-hidden", "true");
    const scaleHead = document.createElement("div");
    scaleHead.className = "tr-scale-head";
    scaleHead.innerHTML = `<span class="tr-badge">TR-REC</span><span class="tr-fx-legend" title="Per-Voice FX Sends: EC = Dub Echo · RV = Reverb · FL = Filter/Drive">EC·RV·FL</span>`;
    scaleRow.append(scaleHead);
    for (let i = 0; i < STEP_COUNT; i++) {
      const cell = document.createElement("div");
      cell.className = `tr-scale-step q${i >> 2}${i % 4 === 0 ? " beat" : ""}`;
      cell.dataset.step = String(i + 1);
      cell.innerHTML = `<span class="tr-step-num">${i + 1}</span><kbd class="tr-step-key">${STEP_KEYS[i]}</kbd>`;
      this.scaleCells.push(cell);
      scaleRow.append(cell);
    }
    root.append(scaleRow);

    VOICES.forEach((voice, r) => {
      const row = document.createElement("div");
      row.className = "row";
      row.dataset.voice = voice.id;
      row.setAttribute("role", "group");
      row.setAttribute("aria-label", voice.name);

      const head = document.createElement("div");
      head.className = "row-head";
      const vKey = VOICE_KEYS[r] ?? "";
      const sel = document.createElement("button");
      sel.type = "button";
      sel.className = "voice-btn";
      sel.textContent = voice.short;
      sel.dataset.select = voice.id;
      sel.dataset.key = vKey;
      sel.title = `${voice.name} (Key ${vKey} select · Shift+${vKey} play · ⌥+${vKey} mute · Ctrl+${vKey} echo)`;
      sel.setAttribute("aria-label", `${voice.name} controls`);
      sel.addEventListener("click", () => handlers.onSelect(voice.id));

      const mute = document.createElement("button");
      mute.type = "button";
      mute.className = "mute";
      mute.textContent = "M";
      mute.dataset.mute = voice.id;
      mute.title = `Mute ${voice.name} (⌥+${vKey})`;
      mute.setAttribute("aria-label", `Mute ${voice.name}`);
      mute.addEventListener("click", () => handlers.onMute(voice.id));

      const fxWrap = document.createElement("div");
      fxWrap.className = "row-fx";
      const rowFxMap = {} as Record<VoiceFxKind, HTMLButtonElement>;
      for (const def of FX_DEFS) {
        const fb = document.createElement("button");
        fb.type = "button";
        fb.className = `fx-led fx-${def.kind}`;
        fb.textContent = def.short;
        fb.dataset.voice = voice.id;
        fb.dataset.voiceFx = def.kind;
        fb.setAttribute("aria-pressed", "false");
        fb.setAttribute("aria-label", `${voice.name} ${def.label}`);
        fb.title =
          def.kind === "echo"
            ? `Toggle ${def.label} on ${voice.name} (Ctrl+${vKey} or select + ⌥+E)`
            : def.kind === "reverb"
              ? `Toggle ${def.label} on ${voice.name} (select + ⌥+R)`
              : `Toggle ${def.label} on ${voice.name} (select + ⌥+D)`;
        fb.addEventListener("click", () => handlers.onToggleFx?.(voice.id, def.kind));
        rowFxMap[def.kind] = fb;
        fxWrap.append(fb);
      }

      head.append(sel, mute, fxWrap);
      row.append(head);
      this.selectButtons.push(sel);
      this.muteButtons.push(mute);
      this.fxButtons.push(rowFxMap);

      const buttons: HTMLButtonElement[] = [];
      for (let i = 0; i < STEP_COUNT; i++) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = `step q${i >> 2}${i % 4 === 0 ? " beat" : ""}`;
        b.dataset.voice = voice.id;
        b.dataset.step = String(i + 1);
        b.dataset.key = STEP_KEYS[i] ?? "";
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

  render(spec: PatternSpec, selected: VoiceId, voiceFx?: VoiceFxMap): void {
    VOICES.forEach((voice, r) => {
      const v = spec.voices[voice.id];
      const steps = normalizeSteps(v.steps);
      const row = this.rows[r]!;
      row.classList.toggle("muted", v.mute);
      row.classList.toggle("selected", voice.id === selected);
      this.selectButtons[r]!.setAttribute("aria-pressed", String(voice.id === selected));
      this.muteButtons[r]!.setAttribute("aria-pressed", String(v.mute));

      const fxState = voiceFx?.[voice.id];
      const fxBtns = this.fxButtons[r]!;
      if (fxState) {
        fxBtns.echo.setAttribute("aria-pressed", String(fxState.echo));
        fxBtns.reverb.setAttribute("aria-pressed", String(fxState.reverb));
        fxBtns.filter.setAttribute("aria-pressed", String(fxState.filter));
        const activeKinds: VoiceFxKind[] = [];
        if (fxState.echo) activeKinds.push("echo");
        if (fxState.reverb) activeKinds.push("reverb");
        if (fxState.filter) activeKinds.push("filter");
        if (activeKinds.length === 0) {
          delete row.dataset.fx;
        } else if (activeKinds.length === 1) {
          row.dataset.fx = activeKinds[0]!;
        } else {
          row.dataset.fx = "multi";
        }
      }

      this.steps[r]!.forEach((b, i) => {
        const c = (steps[i] ?? "-") as StepChar;
        const state = STATE[c];
        if (b.dataset.state !== state) {
          b.dataset.state = state;
          b.setAttribute("aria-pressed", String(c !== "-"));
          b.title = `${voice.name} step ${i + 1} [${STEP_KEYS[i]}]: ${describeStep(c)}`;
        }
      });
    });
  }

  /** Lights the playhead column (`-1` hides it). */
  setPlayhead(step: number): void {
    if (step === this.head) return;
    if (this.head >= 0) {
      this.scaleCells[this.head]?.classList.remove("head");
      for (const row of this.steps) {
        const cell = row[this.head];
        if (cell) {
          cell.classList.remove("head");
          cell.classList.remove("fx-hit");
        }
      }
    }
    this.head = step;
    if (step >= 0) {
      this.scaleCells[step]?.classList.add("head");
      this.steps.forEach((row, r) => {
        const cell = row[step];
        if (!cell) return;
        cell.classList.add("head");
        const rowEl = this.rows[r];
        const hasFx = Boolean(rowEl?.dataset.fx);
        const isHit = cell.dataset.state && cell.dataset.state !== "off";
        if (hasFx && isHit && !rowEl?.classList.contains("muted")) {
          cell.classList.add("fx-hit");
        }
      });
    }
  }

  /** Updates roving tabindex to `(r, c)` and optionally focuses the step button. */
  setFocus(r: number, c: number, focus: boolean): void {
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
