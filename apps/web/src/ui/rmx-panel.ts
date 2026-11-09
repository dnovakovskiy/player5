// Pioneer RMX-1000 Remix Station surface controller:
//   - X-PAD FX (Bank A/B drum pads, interactive 1/1..1/32 touch roll ribbon,
//     Latch Roll, Pitch shift, and live 16-step Overdub recording)
//   - SCENE FX (5 Build Up + 5 Break Down processors with Main + Sub knobs)
//   - ISOLATOR FX (3-Band Low/Mid/High EQ + Kill switches + 4 modes)
//   - RELEASE FX (Spring-loaded Vinyl Brake / Echo / Back Spin exit lever)

import type { AudioEngine } from "../engine/audio-engine";
import {
  defaultRmxState,
  type IsolatorMode,
  type ReleaseMode,
  type RmxState,
  type RollDivision,
  type SceneFxId,
} from "../engine/rmx-fx";
import type { VoiceId } from "../voices";

export interface RmxCallbacks {
  audio: AudioEngine;
  /** Current BPM (followed tempo or internal pattern BPM). */
  getBpm(): number;
  /** Current playing step (0..15, or -1 when stopped). */
  getStep(): number;
  /** Punches a hit into the running pattern when Overdub is enabled. */
  onOverdubHit(voice: VoiceId, step: number, accent: boolean, tuneOffset: number): void;
  /** Toggles mute on a voice in the pattern. */
  onToggleVoiceMute(voice: VoiceId): void;
  /** Clears a voice's steps in the pattern. */
  onClearVoice(voice: VoiceId): void;
  /** Queries whether a voice is currently muted in the pattern. */
  isVoiceMuted(voice: VoiceId): boolean;
}

interface PadDef {
  id: VoiceId;
  short: string;
  label: string;
}

const BANK_A: readonly PadDef[] = [
  { id: "kick", short: "BD", label: "KICK" },
  { id: "snare", short: "SD", label: "SNARE" },
  { id: "clap", short: "CP", label: "CLAP" },
  { id: "closed_hat", short: "CH", label: "HI-HAT" },
];

const BANK_B: readonly PadDef[] = [
  { id: "low_tom", short: "LT", label: "TOM" },
  { id: "rim", short: "RS", label: "RIM" },
  { id: "open_hat", short: "OH", label: "OPEN HAT" },
  { id: "cowbell", short: "CB", label: "COWBELL" },
];

const SCENE_LABELS: Readonly<Record<SceneFxId, string>> = {
  bpf_echo: "BPF ECHO",
  echo: "ECHO",
  noise: "NOISE",
  spiral_up: "SPIRAL UP",
  reverb_up: "REVERB UP",
  hpf_echo: "HPF ECHO",
  lpf_echo: "LPF ECHO",
  crush_echo: "CRUSH ECHO",
  spiral_down: "SPIRAL DOWN",
  reverb_down: "REVERB DOWN",
};

const ROLL_DIVS: readonly RollDivision[] = [1, 2, 4, 8, 16, 32];

export class RmxPanel {
  private state: RmxState = defaultRmxState();
  private rollPointerDown = false;
  private rollLatched = false;
  private overdubStepCursor = 0;

  constructor(
    private readonly root: HTMLElement,
    private readonly cb: RmxCallbacks,
  ) {
    this.cb.audio.setRmxRollOverdubHandler((voice, stepIdx, accent, pitch) => {
      if (this.state.xpadOverdub) {
        this.cb.onOverdubHit(voice, stepIdx, accent, pitch);
      }
    });
    this.bind();
    this.render();
  }

  get current(): RmxState {
    return this.state;
  }

  /** Triggers one of the 4 X-Pad slots (0..3) in the active bank. */
  triggerPadSlot(slot: number): void {
    const bank = this.state.xpadBankB ? BANK_B : BANK_A;
    const pad = bank[slot];
    if (!pad) return;
    this.state.xpadVoice = pad.id;
    this.firePadHit(true);
    this.pushState();
  }

  /** Triggers any of the 10 voices directly through the RMX X-Pad synth & overdub. */
  triggerVoicePad(voice: VoiceId, accent = true): void {
    this.state.xpadVoice = voice;
    this.firePadHit(accent);
    this.pushState();
  }

  /** Engages or releases the spring-loaded Release FX lever from keyboard shortcuts. */
  setReleaseLever(held: boolean): void {
    if (held) {
      if (this.state.releaseHeld) return;
      void this.cb.audio.ensureStarted().catch(() => {});
      this.state.releaseHeld = true;
      this.pushState();
    } else {
      if (!this.state.releaseHeld) return;
      this.state.releaseHeld = false;
      this.state.sceneAmount = 0;
      if (!this.rollLatched) this.stopRoll();
      this.pushState();
    }
  }

  /** Starts a beat-roll division from a held keyboard key. */
  holdRoll(div: RollDivision): void {
    this.startRoll(div);
  }

  /** Ends a keyboard-held beat roll unless Latch Roll is active. */
  releaseRoll(): void {
    if (!this.rollLatched && !this.rollPointerDown) {
      this.stopRoll();
      this.pushState();
    }
  }

  private bind(): void {
    const $ = <T extends HTMLElement>(id: string) => this.root.querySelector<T>(`#${id}`)!;

    // Reset all RMX FX
    $("rmx-reset").addEventListener("click", () => {
      this.stopRoll();
      this.rollLatched = false;
      this.state = defaultRmxState();
      this.pushState();
    });

    // Scene FX buttons (Build Up + Break Down)
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-scene-fx]")) {
      btn.addEventListener("click", () => {
        const fx = btn.dataset.sceneFx as SceneFxId;
        this.state.sceneFx = fx;
        this.pushState();
      });
    }

    const sceneAmt = $<HTMLInputElement>("rmx-scene-amount");
    sceneAmt.addEventListener("input", () => {
      this.state.sceneAmount = Number(sceneAmt.value);
      // If user turns up NOISE or any Scene FX while audio is idle, boot AudioContext so NOISE is audible
      if (this.state.sceneAmount > 0.02) {
        void this.cb.audio.ensureStarted().catch(() => {});
      }
      this.pushState();
    });

    const sceneSub = $<HTMLInputElement>("rmx-scene-sub");
    sceneSub.addEventListener("input", () => {
      this.state.sceneSub = Number(sceneSub.value);
      this.pushState();
    });

    // Isolator FX modes + bands + kills
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-iso-mode]")) {
      btn.addEventListener("click", () => {
        this.state.isoMode = btn.dataset.isoMode as IsolatorMode;
        this.pushState();
      });
    }

    const bindIso = (id: string, key: "isoLow" | "isoMid" | "isoHigh") => {
      const el = $<HTMLInputElement>(id);
      el.addEventListener("input", () => {
        this.state[key] = Number(el.value);
        this.pushState();
      });
      el.addEventListener("dblclick", () => {
        this.state[key] = 0;
        this.pushState();
      });
    };
    bindIso("rmx-iso-low", "isoLow");
    bindIso("rmx-iso-mid", "isoMid");
    bindIso("rmx-iso-high", "isoHigh");

    const bindKill = (id: string, key: "killLow" | "killMid" | "killHigh") => {
      $(id).addEventListener("click", () => {
        this.state[key] = !this.state[key];
        this.pushState();
      });
    };
    bindKill("rmx-kill-low", "killLow");
    bindKill("rmx-kill-mid", "killMid");
    bindKill("rmx-kill-high", "killHigh");

    // X-Pad Quantize & Bank A/B toggles
    const qBtn = this.root.querySelector<HTMLButtonElement>("#rmx-quantize");
    if (qBtn) {
      qBtn.addEventListener("click", () => {
        this.state.xpadQuantize = !(this.state.xpadQuantize !== false);
        this.pushState();
      });
    }

    $("rmx-bank").addEventListener("click", () => {
      this.state.xpadBankB = !this.state.xpadBankB;
      const bank = this.state.xpadBankB ? BANK_B : BANK_A;
      if (!bank.some((p) => p.id === this.state.xpadVoice)) {
        this.state.xpadVoice = bank[0]!.id;
      }
      this.pushState();
    });

    // X-Pad 4 Voice Pads
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-xpad-slot]")) {
      btn.addEventListener("click", () => {
        const slot = Number(btn.dataset.xpadSlot);
        const bank = this.state.xpadBankB ? BANK_B : BANK_A;
        const pad = bank[slot];
        if (!pad) return;
        this.state.xpadVoice = pad.id;
        this.firePadHit(true);
        this.pushState();
      });
    }

    // X-Pad Pitch
    const pitch = $<HTMLInputElement>("rmx-pitch");
    pitch.addEventListener("input", () => {
      this.state.xpadPitch = Number(pitch.value);
      this.pushState();
    });
    pitch.addEventListener("dblclick", () => {
      this.state.xpadPitch = 0;
      this.pushState();
    });

    // X-Pad Overdub, Latch Roll, Voice Mute, Voice Clear
    $("rmx-overdub").addEventListener("click", () => {
      this.state.xpadOverdub = !this.state.xpadOverdub;
      this.pushState();
    });

    $("rmx-roll-latch").addEventListener("click", () => {
      this.rollLatched = !this.rollLatched;
      if (this.rollLatched) {
        this.startRoll(this.state.xpadRollDiv);
      } else if (!this.rollPointerDown) {
        this.stopRoll();
      }
      this.pushState();
    });

    $("rmx-voice-mute").addEventListener("click", () => {
      this.cb.onToggleVoiceMute(this.state.xpadVoice);
      this.render();
    });

    $("rmx-voice-clear").addEventListener("click", () => {
      this.cb.onClearVoice(this.state.xpadVoice);
      this.render();
    });

    // X-Pad Touch Roll Strip (6 division buttons + continuous drag across strip)
    const strip = $("rmx-xpad-strip");
    const divFromPoint = (clientX: number): RollDivision => {
      const rect = strip.getBoundingClientRect();
      if (rect.width <= 0) return this.state.xpadRollDiv;
      const ratio = Math.max(0, Math.min(0.999, (clientX - rect.left) / rect.width));
      const idx = Math.floor(ratio * ROLL_DIVS.length);
      return ROLL_DIVS[idx] ?? 8;
    };

    for (const btn of strip.querySelectorAll<HTMLButtonElement>("[data-roll-div]")) {
      btn.addEventListener("pointerdown", (e) => {
        e.preventDefault();
        const div = Number(btn.dataset.rollDiv) as RollDivision;
        this.rollPointerDown = true;
        this.startRoll(div);
      });
      btn.addEventListener("click", (e) => {
        // Keyboard Enter/Space activation on a division button triggers one roll burst
        if (e.detail === 0) {
          const div = Number(btn.dataset.rollDiv) as RollDivision;
          this.state.xpadRollDiv = div;
          this.firePadHit(div >= 16);
          this.pushState();
        }
      });
    }

    strip.addEventListener("pointermove", (e) => {
      if (!this.rollPointerDown) return;
      const div = divFromPoint(e.clientX);
      if (div !== this.state.xpadRollDiv) {
        this.state.xpadRollDiv = div;
        this.pushState();
      }
    });

    const endPointerRoll = () => {
      if (!this.rollPointerDown) return;
      this.rollPointerDown = false;
      if (!this.rollLatched) {
        this.stopRoll();
        this.pushState();
      }
    };
    window.addEventListener("pointerup", endPointerRoll);
    window.addEventListener("pointercancel", endPointerRoll);

    // Release FX modes + spring-loaded lever
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-release-mode]")) {
      btn.addEventListener("click", () => {
        this.state.releaseMode = btn.dataset.releaseMode as ReleaseMode;
        this.pushState();
      });
    }

    const lever = $("rmx-release-lever");
    const engageLever = () => {
      if (this.state.releaseHeld) return;
      void this.cb.audio.ensureStarted().catch(() => {});
      this.state.releaseHeld = true;
      this.pushState();
    };
    const releaseLever = () => {
      if (!this.state.releaseHeld) return;
      this.state.releaseHeld = false;
      // Like hardware RMX-1000: releasing the exit lever clears Scene FX build-up
      this.state.sceneAmount = 0;
      if (!this.rollLatched) this.stopRoll();
      this.pushState();
    };

    lever.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      engageLever();
    });
    lever.addEventListener("keydown", (e) => {
      if ((e.key === "Enter" || e.key === "r" || e.key === "R") && !e.repeat) {
        engageLever();
      }
    });
    lever.addEventListener("keyup", (e) => {
      if (e.key === "Enter" || e.key === "r" || e.key === "R") {
        releaseLever();
      }
    });
    window.addEventListener("pointerup", releaseLever);
    window.addEventListener("pointercancel", releaseLever);
  }

  private startRoll(div: RollDivision): void {
    this.state.xpadRollDiv = div;
    this.state.xpadRolling = true;
    void this.cb.audio.ensureStarted().catch(() => {});
    this.pushState();
  }

  private stopRoll(): void {
    this.state.xpadRolling = false;
  }

  private firePadHit(accent: boolean): void {
    const velocity = accent ? 1.0 : 0.85;
    const voice = this.state.xpadVoice;
    const pitch = this.state.xpadPitch;
    const overdub = this.state.xpadOverdub;
    const liveStep = this.cb.getStep();
    // If Overdub is armed, record immediately when stopped (or using current/quantized step when playing)
    if (overdub && liveStep < 0) {
      const targetStep = this.overdubStepCursor++ % 16;
      this.cb.onOverdubHit(voice, targetStep, accent, pitch);
    }
    void this.cb.audio.triggerRmxPad(voice, pitch, velocity).then((quantizedStep) => {
      if (overdub && liveStep >= 0) {
        const targetStep = quantizedStep >= 0 ? quantizedStep : liveStep;
        this.cb.onOverdubHit(voice, targetStep, accent, pitch);
      }
    });
  }

  private pushState(): void {
    this.cb.audio.setRmxState(this.state);
    this.render();
  }

  render(): void {
    const $ = <T extends HTMLElement>(id: string) => this.root.querySelector<T>(`#${id}`)!;
    const s = this.state;

    // Header readout
    const fxName = s.sceneFx ? SCENE_LABELS[s.sceneFx] : "BYPASS";
    const fxPct = `${Math.round(s.sceneAmount * 100)}%`;
    const bank = s.xpadBankB ? BANK_B : BANK_A;
    const activePad = bank.find((p) => p.id === s.xpadVoice) ?? BANK_A[0]!;
    const qTag = s.xpadQuantize !== false ? " · QUANT" : "";
    $("rmx-readout").textContent = s.releaseHeld
      ? `RELEASE · ${s.releaseMode.replace("_", " ").toUpperCase()}`
      : `${fxName} ${fxPct} · X-PAD ${activePad.label} 1/${s.xpadRollDiv}${qTag}`;

    // Scene FX buttons & sliders
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-scene-fx]")) {
      const on = btn.dataset.sceneFx === s.sceneFx;
      btn.setAttribute("aria-pressed", String(on));
    }
    const sceneAmt = $<HTMLInputElement>("rmx-scene-amount");
    if (sceneAmt.value !== String(s.sceneAmount)) sceneAmt.value = String(s.sceneAmount);
    $("rmx-scene-amount-out").textContent = fxPct;

    const sceneSub = $<HTMLInputElement>("rmx-scene-sub");
    if (sceneSub.value !== String(s.sceneSub)) sceneSub.value = String(s.sceneSub);
    $("rmx-scene-sub-out").textContent = `${Math.round(s.sceneSub * 100)}%`;

    // Isolator FX
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-iso-mode]")) {
      btn.setAttribute("aria-pressed", String(btn.dataset.isoMode === s.isoMode));
    }
    const fmtIso = (db: number, kill: boolean) =>
      kill || db <= -47.5 ? "KILL" : `${db > 0 ? "+" : ""}${Math.round(db)} dB`;

    const isoLow = $<HTMLInputElement>("rmx-iso-low");
    if (isoLow.value !== String(s.isoLow)) isoLow.value = String(s.isoLow);
    $("rmx-iso-low-out").textContent = fmtIso(s.isoLow, s.killLow);
    $("rmx-kill-low").setAttribute("aria-pressed", String(s.killLow));

    const isoMid = $<HTMLInputElement>("rmx-iso-mid");
    if (isoMid.value !== String(s.isoMid)) isoMid.value = String(s.isoMid);
    $("rmx-iso-mid-out").textContent = fmtIso(s.isoMid, s.killMid);
    $("rmx-kill-mid").setAttribute("aria-pressed", String(s.killMid));

    const isoHigh = $<HTMLInputElement>("rmx-iso-high");
    if (isoHigh.value !== String(s.isoHigh)) isoHigh.value = String(s.isoHigh);
    $("rmx-iso-high-out").textContent = fmtIso(s.isoHigh, s.killHigh);
    $("rmx-kill-high").setAttribute("aria-pressed", String(s.killHigh));

    // X-Pad Quantize, Bank & Voice Pads
    const qBtn = this.root.querySelector<HTMLButtonElement>("#rmx-quantize");
    if (qBtn) {
      qBtn.setAttribute("aria-pressed", String(s.xpadQuantize !== false));
    }
    const bankBtn = $("rmx-bank");
    bankBtn.textContent = s.xpadBankB ? "Bank B (LT/RS/OH/CB)" : "Bank A (BD/SD/CP/CH)";
    bankBtn.setAttribute("aria-pressed", String(s.xpadBankB));

    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-xpad-slot]")) {
      const slot = Number(btn.dataset.xpadSlot);
      const pad = bank[slot];
      if (!pad) continue;
      btn.textContent = `${pad.short} · ${pad.label}`;
      btn.setAttribute("aria-pressed", String(pad.id === s.xpadVoice));
      btn.dataset.voice = pad.id;
    }

    const pitch = $<HTMLInputElement>("rmx-pitch");
    if (pitch.value !== String(s.xpadPitch)) pitch.value = String(s.xpadPitch);
    const semi = Math.round(s.xpadPitch * 12);
    $("rmx-pitch-out").textContent = `${semi > 0 ? "+" : ""}${semi} st`;

    $("rmx-overdub").setAttribute("aria-pressed", String(s.xpadOverdub));
    $("rmx-roll-latch").setAttribute("aria-pressed", String(this.rollLatched));
    const muted = this.cb.isVoiceMuted(s.xpadVoice);
    $("rmx-voice-mute").setAttribute("aria-pressed", String(muted));

    // X-Pad Roll Strip divisions
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-roll-div]")) {
      const div = Number(btn.dataset.rollDiv);
      const active = div === s.xpadRollDiv;
      btn.setAttribute("aria-pressed", String(active));
      btn.classList.toggle("rolling", active && s.xpadRolling);
    }

    // Release FX
    for (const btn of this.root.querySelectorAll<HTMLButtonElement>("[data-release-mode]")) {
      btn.setAttribute("aria-pressed", String(btn.dataset.releaseMode === s.releaseMode));
    }
    const lever = $("rmx-release-lever");
    lever.setAttribute("aria-pressed", String(s.releaseHeld));
    lever.textContent = s.releaseHeld ? "RELEASE FX ENGAGED (RELEASE TO DROP)" : "PULL RELEASE LEVER (HOLD)";
  }
}
