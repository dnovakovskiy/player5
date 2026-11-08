// 3Blue1Brown-style interactive visual explainer, step-by-step quick-start
// guide, and MacBook Air keyboard map for player5.

export interface Chapter {
  id: number;
  title: string;
  startSec: number;
  endSec: number;
}

export const EXPLAINER_DURATION_SEC = 120;

export const CHAPTERS: readonly Chapter[] = [
  {
    id: 0,
    title: "1 · The 16-Step Circle & Home Row",
    startSec: 0,
    endSec: 30,
  },
  {
    id: 1,
    title: "2 · 10 Drum Voices & Sound Kits",
    startSec: 30,
    endSec: 60,
  },
  {
    id: 2,
    title: "3 · Shaping Analog Drum Sounds",
    startSec: 60,
    endSec: 90,
  },
  {
    id: 3,
    title: "4 · RMX-1000 FX & DJ Booth Sync",
    startSec: 90,
    endSec: 120,
  },
];

interface Caption {
  from: number;
  to: number;
  text: string;
}

const CAPTIONS: readonly Caption[] = [
  {
    from: 0,
    to: 10,
    text: "A 16-step pattern is one musical bar of 4 beats, divided into 16 sixteenth-note steps that repeat in a continuous loop.",
  },
  {
    from: 10,
    to: 20,
    text: "Unrolling that circle onto your MacBook Air keyboard puts Steps 1–8 on the Home Row (A S D F G H J K) and Steps 9–16 on the row below (Z X C V B N M ,).",
  },
  {
    from: 20,
    to: 30,
    text: "Tap any step key once for a normal hit, twice for a louder Accent, and a third time to clear it. Hold Shift + a step key to add a double-hit Flam.",
  },
  {
    from: 30,
    to: 40,
    text: "The number keys 1 through 0 select which of the 10 drum voices you are editing: Kick, Snare, Low/Mid/High Toms, Rimshot, Clap, Closed/Open Hats, and Cowbell.",
  },
  {
    from: 40,
    to: 50,
    text: "Hold Shift + 1..0 to play any drum sound live with your fingers, or hold Option (⌥) + 1..0 to mute and unmute that drum in the mix.",
  },
  {
    from: 50,
    to: 60,
    text: "Press Y, U, I, O, or P (or use the Drum kit menu) to swap the entire analog drum machine sound character without losing your step pattern.",
  },
  {
    from: 60,
    to: 70,
    text: "Every drum voice is synthesized in real time. Adjusting Tune and Decay on the Kick controls how deep the bass pitch drops and how long the sub-bass rings.",
  },
  {
    from: 70,
    to: 80,
    text: "Closed and Open Hi-Hats blend 6 metallic oscillators—and just like a real hi-hat pedal, playing a Closed Hat automatically chokes a ringing Open Hat.",
  },
  {
    from: 80,
    to: 90,
    text: "The Snare and Hand-Clap let you dial in wire snap and room decay, while the Shuffle slider delays every second 16th-note step for house and disco swing.",
  },
  {
    from: 90,
    to: 100,
    text: "Press the Backquote (`) key (or click RMX-1000 Station) to switch into live performance FX mode. Keys Q, W, E, R trigger the 4 X-Pad drum pads.",
  },
  {
    from: 100,
    to: 110,
    text: "Hold R at any time to pull the spring-loaded Release FX lever (Vinyl Brake, Echo Out, or Back Spin). Let go of R to drop cleanly back into the beat.",
  },
  {
    from: 110,
    to: 120,
    text: "To lock in with a DJ set, press Space to Play/Stop, T to tap tempo, [ and ] to nudge BPM, and ; or ' to align the beat phase by ±1 ms.",
  },
];

// 3Blue1Brown visual palette
const C_BG = "#0b0d14";
const C_GRID = "#162032";
const C_AXIS = "#2b3b55";
const C_BLUE = "#58c4dd";
const C_YELLOW = "#f7d154";
const C_RED = "#fc6255";
const C_GREEN = "#83c167";
const C_PURPLE = "#b189c6";
const C_TEAL = "#5cd0b3";
const C_TEXT = "#ecebe6";
const C_MUTED = "#8e96a8";

const clamp01 = (x: number) => Math.max(0, Math.min(1, x));
const smoothstep = (a: number, b: number, x: number) => {
  const t = clamp01((x - a) / (b - a));
  return t * t * (3 - 2 * t);
};

interface KeyCap {
  code: string;
  label: string;
  sub: string;
  color: string;
}

const MACBOOK_ROWS: readonly (readonly KeyCap[])[] = [
  [
    { code: "Backquote", label: "`", sub: "RMX Mode", color: C_PURPLE },
    { code: "Digit1", label: "1", sub: "BD Kick", color: C_BLUE },
    { code: "Digit2", label: "2", sub: "SD Snare", color: C_BLUE },
    { code: "Digit3", label: "3", sub: "LT Tom", color: C_BLUE },
    { code: "Digit4", label: "4", sub: "MT Tom", color: C_BLUE },
    { code: "Digit5", label: "5", sub: "HT Tom", color: C_BLUE },
    { code: "Digit6", label: "6", sub: "RS Rim", color: C_BLUE },
    { code: "Digit7", label: "7", sub: "CP Clap", color: C_BLUE },
    { code: "Digit8", label: "8", sub: "CH Hat", color: C_BLUE },
    { code: "Digit9", label: "9", sub: "OH Hat", color: C_BLUE },
    { code: "Digit0", label: "0", sub: "CB Bell", color: C_BLUE },
    { code: "Backspace", label: "⌫", sub: "Clear", color: C_RED },
  ],
  [
    { code: "KeyQ", label: "Q", sub: "Prev / Pad1", color: C_PURPLE },
    { code: "KeyW", label: "W", sub: "Next / Pad2", color: C_PURPLE },
    { code: "KeyE", label: "E", sub: "Flam / Pad3", color: C_PURPLE },
    { code: "KeyR", label: "R", sub: "Release FX", color: C_RED },
    { code: "KeyT", label: "T", sub: "Tap BPM", color: C_YELLOW },
    { code: "KeyY", label: "Y", sub: "Sub Kit", color: C_TEAL },
    { code: "KeyU", label: "U", sub: "Punch Kit", color: C_TEAL },
    { code: "KeyI", label: "I", sub: "Disco Kit", color: C_TEAL },
    { code: "KeyO", label: "O", sub: "Silver Kit", color: C_TEAL },
    { code: "KeyP", label: "P", sub: "Raw Kit", color: C_TEAL },
    { code: "BracketLeft", label: "[", sub: "BPM −", color: C_YELLOW },
    { code: "BracketRight", label: "]", sub: "BPM +", color: C_YELLOW },
  ],
  [
    { code: "KeyA", label: "A", sub: "Step 1", color: C_GREEN },
    { code: "KeyS", label: "S", sub: "Step 2", color: C_GREEN },
    { code: "KeyD", label: "D", sub: "Step 3", color: C_GREEN },
    { code: "KeyF", label: "F", sub: "Step 4", color: C_GREEN },
    { code: "KeyG", label: "G", sub: "Step 5", color: C_GREEN },
    { code: "KeyH", label: "H", sub: "Step 6", color: C_GREEN },
    { code: "KeyJ", label: "J", sub: "Step 7", color: C_GREEN },
    { code: "KeyK", label: "K", sub: "Step 8", color: C_GREEN },
    { code: "KeyL", label: "L", sub: "Re-sync", color: C_YELLOW },
    { code: "Semicolon", label: ";", sub: "Nudge −", color: C_YELLOW },
    { code: "Quote", label: "'", sub: "Nudge +", color: C_YELLOW },
  ],
  [
    { code: "KeyZ", label: "Z", sub: "Step 9", color: C_GREEN },
    { code: "KeyX", label: "X", sub: "Step 10", color: C_GREEN },
    { code: "KeyC", label: "C", sub: "Step 11", color: C_GREEN },
    { code: "KeyV", label: "V", sub: "Step 12", color: C_GREEN },
    { code: "KeyB", label: "B", sub: "Step 13", color: C_GREEN },
    { code: "KeyN", label: "N", sub: "Step 14", color: C_GREEN },
    { code: "KeyM", label: "M", sub: "Step 15", color: C_GREEN },
    { code: "Comma", label: ",", sub: "Step 16", color: C_GREEN },
    { code: "Period", label: ".", sub: "Mute Voice", color: C_RED },
    { code: "Slash", label: "?", sub: "How To", color: C_YELLOW },
  ],
];

export class HowToExplainer {
  private readonly modal: HTMLElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly ctx2d: CanvasRenderingContext2D;
  private readonly playBtn: HTMLButtonElement;
  private readonly scrub: HTMLInputElement;
  private readonly timeOut: HTMLElement;
  private readonly captionEl: HTMLElement;
  private readonly soundBtn: HTMLButtonElement;
  private readonly speedBtn: HTMLButtonElement;
  private readonly keyFeedbackEl: HTMLElement;
  private readonly chapterBtns: HTMLButtonElement[];

  private openState = false;
  private playing = false;
  private currentSec = 0;
  private speed = 1;
  private soundOn = false;
  private lastFrameMs = 0;
  private rafId = 0;
  private lastChimeStep = -1;
  private activeCodes = new Set<string>();
  private audioCtx: AudioContext | null = null;

  constructor(root: HTMLElement) {
    this.modal = root.querySelector<HTMLElement>("#how-to-modal")!;
    this.canvas = root.querySelector<HTMLCanvasElement>("#manim-canvas")!;
    this.ctx2d = this.canvas.getContext("2d")!;
    this.playBtn = root.querySelector<HTMLButtonElement>("#howto-play")!;
    this.scrub = root.querySelector<HTMLInputElement>("#howto-scrub")!;
    this.timeOut = root.querySelector<HTMLElement>("#howto-time")!;
    this.captionEl = root.querySelector<HTMLElement>("#howto-caption")!;
    this.soundBtn = root.querySelector<HTMLButtonElement>("#howto-sound")!;
    this.speedBtn = root.querySelector<HTMLButtonElement>("#howto-speed")!;
    this.keyFeedbackEl = root.querySelector<HTMLElement>("#howto-live-key")!;
    this.chapterBtns = [...root.querySelectorAll<HTMLButtonElement>("[data-howto-chapter]")];

    this.bind(root);
    this.renderUi();
  }

  get isOpen(): boolean {
    return this.openState;
  }

  open(): void {
    if (this.openState) return;
    this.openState = true;
    this.modal.hidden = false;
    this.playing = true;
    this.lastFrameMs = performance.now();
    this.scheduleFrame();
    this.renderUi();
  }

  close(): void {
    if (!this.openState) return;
    this.openState = false;
    this.playing = false;
    this.modal.hidden = true;
    if (this.rafId) {
      cancelAnimationFrame(this.rafId);
      this.rafId = 0;
    }
    this.renderUi();
  }

  toggle(): void {
    if (this.openState) this.close();
    else this.open();
  }

  /** Highlights a pressed MacBook Air key live in the explainer diagram. */
  notifyKeyDown(code: string, description: string): void {
    this.activeCodes.add(code);
    if (this.openState && description) {
      this.keyFeedbackEl.textContent = `Live Key: ${description}`;
      this.drawFrame();
    }
  }

  notifyKeyUp(code: string): void {
    this.activeCodes.delete(code);
    if (this.openState) this.drawFrame();
  }

  private bind(root: HTMLElement): void {
    root.querySelector<HTMLButtonElement>("#how-to-btn")?.addEventListener("click", () => this.toggle());
    root.querySelector<HTMLButtonElement>("#howto-close")?.addEventListener("click", () => this.close());
    this.modal.addEventListener("click", (e) => {
      if (e.target === this.modal) this.close();
    });

    this.playBtn.addEventListener("click", () => {
      this.playing = !this.playing;
      if (this.playing) {
        if (this.currentSec >= EXPLAINER_DURATION_SEC - 0.1) this.currentSec = 0;
        this.lastFrameMs = performance.now();
        this.scheduleFrame();
      }
      this.renderUi();
    });

    this.scrub.addEventListener("input", () => {
      this.currentSec = Number(this.scrub.value);
      this.renderUi();
      this.drawFrame();
    });

    for (const btn of this.chapterBtns) {
      btn.addEventListener("click", () => {
        const idx = Number(btn.dataset.howtoChapter);
        const ch = CHAPTERS[idx];
        if (!ch) return;
        this.currentSec = ch.startSec;
        this.playing = true;
        this.lastFrameMs = performance.now();
        this.scheduleFrame();
        this.renderUi();
        this.drawFrame();
      });
    }

    this.speedBtn.addEventListener("click", () => {
      this.speed =
        this.speed === 1 ? 0.75 : this.speed === 0.75 ? 0.5 : this.speed === 0.5 ? 1.25 : 1;
      this.speedBtn.textContent = `${this.speed}×`;
    });

    this.soundBtn.addEventListener("click", () => {
      this.soundOn = !this.soundOn;
      this.soundBtn.setAttribute("aria-pressed", String(this.soundOn));
      this.soundBtn.textContent = this.soundOn ? "Chimes: ON" : "Chimes: OFF";
      if (this.soundOn) this.playChime(528, 0.12);
    });

    // Clicking any key cap in the interactive legend triggers its preview
    for (const cap of this.modal.querySelectorAll<HTMLElement>("[data-key-code]")) {
      cap.addEventListener("click", () => {
        const code = cap.dataset.keyCode ?? "";
        const label = cap.dataset.keyDesc ?? code;
        this.keyFeedbackEl.textContent = `Key: ${label}`;
        this.activeCodes.add(code);
        this.drawFrame();
        setTimeout(() => {
          this.activeCodes.delete(code);
          if (this.openState) this.drawFrame();
        }, 450);
      });
    }
  }

  private scheduleFrame(): void {
    if (!this.rafId && this.openState) {
      this.rafId = requestAnimationFrame((now) => this.tick(now));
    }
  }

  private tick(now: number): void {
    this.rafId = 0;
    if (!this.openState) return;
    if (this.playing) {
      const dt = Math.min(0.1, Math.max(0, (now - this.lastFrameMs) / 1000));
      this.lastFrameMs = now;
      this.currentSec += dt * this.speed;
      if (this.currentSec >= EXPLAINER_DURATION_SEC) {
        this.currentSec = 0;
      }
      const chimeIdx = Math.floor(this.currentSec * 0.5);
      if (this.soundOn && chimeIdx !== this.lastChimeStep) {
        this.lastChimeStep = chimeIdx;
        const scale = [261.6, 329.6, 392.0, 523.25, 659.25];
        const freq = scale[chimeIdx % scale.length]!;
        if (chimeIdx % 2 === 0) this.playChime(freq, 0.05);
      }
      this.renderUi();
    }
    this.drawFrame();
    if (this.playing) {
      this.scheduleFrame();
    }
  }

  private playChime(freq: number, gainVal: number): void {
    try {
      const Ctor =
        window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) return;
      this.audioCtx ??= new Ctor();
      if (this.audioCtx.state === "suspended") void this.audioCtx.resume();
      const now = this.audioCtx.currentTime;
      const osc = this.audioCtx.createOscillator();
      const g = this.audioCtx.createGain();
      osc.type = "sine";
      osc.frequency.setValueAtTime(freq, now);
      g.gain.setValueAtTime(gainVal, now);
      g.gain.exponentialRampToValueAtTime(0.0005, now + 0.35);
      osc.connect(g);
      g.connect(this.audioCtx.destination);
      osc.start(now);
      osc.stop(now + 0.36);
    } catch {
      /* ignore audio context restrictions */
    }
  }

  private formatTime(sec: number): string {
    const s = Math.max(0, Math.floor(sec));
    const m = Math.floor(s / 60);
    const rem = s % 60;
    return `${m}:${rem < 10 ? "0" : ""}${rem}`;
  }

  private renderUi(): void {
    this.playBtn.textContent = this.playing ? "Pause" : "Play";
    this.playBtn.setAttribute("aria-pressed", String(this.playing));
    this.scrub.value = this.currentSec.toFixed(2);
    this.timeOut.textContent = `${this.formatTime(this.currentSec)} / ${this.formatTime(EXPLAINER_DURATION_SEC)}`;

    const ch =
      CHAPTERS.find((c) => this.currentSec >= c.startSec && this.currentSec < c.endSec) ?? CHAPTERS[0]!;
    for (const [idx, btn] of this.chapterBtns.entries()) {
      btn.setAttribute("aria-pressed", String(idx === ch.id));
    }

    const cap =
      CAPTIONS.find((c) => this.currentSec >= c.from && this.currentSec < c.to) ??
      CAPTIONS[CAPTIONS.length - 1]!;
    if (this.captionEl.textContent !== cap.text) {
      this.captionEl.textContent = cap.text;
    }
  }

  private drawFrame(): void {
    const ctx = this.ctx2d;
    const W = 720;
    const H = 340;
    if (this.canvas.width !== W || this.canvas.height !== H) {
      this.canvas.width = W;
      this.canvas.height = H;
    }

    const t = this.currentSec;

    // 1. Deep Manim background + subtle coordinate grid
    ctx.fillStyle = C_BG;
    ctx.fillRect(0, 0, W, H);

    ctx.strokeStyle = C_GRID;
    ctx.lineWidth = 1;
    const gridStep = 36;
    for (let x = gridStep; x < W; x += gridStep) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, H);
      ctx.stroke();
    }
    for (let y = gridStep; y < H; y += gridStep) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(W, y);
      ctx.stroke();
    }

    // 2. Dispatch by Chapter (30 seconds per chapter)
    if (t < 30) {
      this.drawChapter1PhaseCircle(ctx, W, H, t);
    } else if (t < 60) {
      this.drawChapter2VoiceMatrix(ctx, W, H, t - 30);
    } else if (t < 90) {
      this.drawChapter3AnalogDsp(ctx, W, H, t - 60);
    } else {
      this.drawChapter4RmxAndSync(ctx, W, H, t - 90);
    }
  }

  /**
   * Chapter 1 (0..30s): The 16-Step Bar Circle gently unrolling onto the
   * MacBook Air Home Row (A..K) & Bottom Row (Z..,).
   */
  private drawChapter1PhaseCircle(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    const cx = 165;
    const cy = 170;
    const R = 94;
    // Slow, unhurried 9-second morph starting at 8s
    const morph = smoothstep(8, 17, localT);

    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("16 Steps in One Bar  →  MacBook Air Home & Bottom Rows", 24, 32);

    ctx.strokeStyle = C_AXIS;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(cx - R - 20, cy);
    ctx.lineTo(cx + R + 20, cy);
    ctx.moveTo(cx, cy - R - 20);
    ctx.lineTo(cx, cy + R + 20);
    ctx.stroke();

    ctx.strokeStyle = C_BLUE;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(cx, cy, R, 0, Math.PI * 2);
    ctx.stroke();

    // Gentle playhead sweep (1 bar every ~6.2 seconds so the eye can follow every step)
    const phase = (localT * 0.16) % 1;
    const activeStep = Math.floor(phase * 16);
    const angle = phase * Math.PI * 2 - Math.PI / 2;
    const vx = cx + Math.cos(angle) * R;
    const vy = cy + Math.sin(angle) * R;

    ctx.strokeStyle = C_YELLOW;
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.lineTo(vx, vy);
    ctx.stroke();

    ctx.fillStyle = C_YELLOW;
    ctx.beginPath();
    ctx.arc(vx, vy, 5, 0, Math.PI * 2);
    ctx.fill();

    const stepKeys = ["A", "S", "D", "F", "G", "H", "J", "K", "Z", "X", "C", "V", "B", "N", "M", ","];
    const hitSteps = new Set([0, 4, 8, 12, 2, 6, 10, 14]);
    const accentSteps = new Set([0, 8]);

    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * Math.PI * 2 - Math.PI / 2;
      const sx = cx + Math.cos(a) * R;
      const sy = cy + Math.sin(a) * R;

      const row = i < 8 ? 0 : 1;
      const col = i % 8;
      const tx = 345 + col * 44 + row * 12;
      const ty = 130 + row * 68;

      const x = sx + (tx - sx) * morph;
      const y = sy + (ty - sy) * morph;
      const isCurrent = i === activeStep;
      const isAccent = accentSteps.has(i);
      const isHit = hitSteps.has(i);

      if (morph > 0.03 && morph < 0.98) {
        ctx.strokeStyle = "rgba(88, 196, 221, 0.22)";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(sx, sy);
        ctx.lineTo(x, y);
        ctx.stroke();
      }

      const boxW = 36;
      const boxH = 38;
      ctx.fillStyle = isCurrent
        ? "rgba(247, 209, 84, 0.28)"
        : isAccent
          ? "rgba(252, 98, 85, 0.22)"
          : isHit
            ? "rgba(131, 193, 103, 0.18)"
            : "rgba(22, 32, 50, 0.85)";
      ctx.strokeStyle = isCurrent ? C_YELLOW : isAccent ? C_RED : isHit ? C_GREEN : C_BLUE;
      ctx.lineWidth = isCurrent ? 2.4 : 1.5;

      ctx.beginPath();
      ctx.roundRect(x - boxW / 2, y - boxH / 2, boxW, boxH, 6);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = C_TEXT;
      ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(stepKeys[i]!, x, y - 2);

      ctx.fillStyle = isCurrent ? C_YELLOW : C_MUTED;
      ctx.font = "10px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(String(i + 1), x, y + 13);
    }
    ctx.textAlign = "left";

    if (morph > 0.35) {
      ctx.fillStyle = C_GREEN;
      ctx.font = 'italic 14px Georgia, "Times New Roman", serif';
      ctx.fillText("Home Row (Steps 1–8):  A  S  D  F  G  H  J  K", 345, 92);
      ctx.fillText("Bottom Row (Steps 9–16):  Z  X  C  V  B  N  M  ,", 355, 244);
    }

    if (localT > 18) {
      ctx.fillStyle = C_YELLOW;
      ctx.font = 'italic 14.5px Georgia, "Times New Roman", serif';
      ctx.fillText(
        "Tap key:  Off → Hit → Accent     |     Hold Shift + key:  Add Flam (double hit)",
        82,
        312,
      );
    }
  }

  /**
   * Chapter 2 (30..60s): Number Row 1..0 → 10 Voices, Shift Audition,
   * Option Mute, and Y U I O P Analog Kits.
   */
  private drawChapter2VoiceMatrix(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Number Row [1 – 0] Selects Drum Voices  ·  [Y U I O P] Swaps Kits", 24, 32);

    const voices = ["BD", "SD", "LT", "MT", "HT", "RS", "CP", "CH", "OH", "CB"];
    const names = ["Kick", "Snare", "LoTom", "MidTom", "HiTom", "Rim", "Clap", "ClHat", "OpHat", "Bell"];
    const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    // Calm scan: ~2.2 seconds per drum voice
    const scanIdx = Math.floor((localT * 0.45) % 10);

    for (let i = 0; i < 10; i++) {
      const x = 44 + i * 64;
      const y = 68;
      const active = i === scanIdx || this.activeCodes.has(`Digit${keys[i]}`);

      ctx.fillStyle = active ? "rgba(88, 196, 221, 0.26)" : "rgba(22, 32, 50, 0.88)";
      ctx.strokeStyle = active ? C_YELLOW : C_BLUE;
      ctx.lineWidth = active ? 2.4 : 1.4;
      ctx.beginPath();
      ctx.roundRect(x, y, 54, 62, 7);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = C_TEXT;
      ctx.font = "bold 15px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(keys[i]!, x + 27, y + 20);

      ctx.fillStyle = active ? C_YELLOW : C_BLUE;
      ctx.font = "bold 11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(voices[i]!, x + 27, y + 38);

      ctx.fillStyle = C_MUTED;
      ctx.font = "9.5px system-ui, sans-serif";
      ctx.fillText(names[i]!, x + 27, y + 53);
    }

    const modY = 156;
    const boxes = [
      {
        title: "Press  [1 .. 0]",
        line1: "Selects that drum row",
        line2: "for step editing & knobs",
        color: C_BLUE,
      },
      {
        title: "Hold Shift + [1 .. 0]",
        line1: "Plays the drum sound live",
        line2: "(finger-drumming / preview)",
        color: C_YELLOW,
      },
      {
        title: "Hold ⌥ Option + [1 .. 0]",
        line1: "Mutes or unmutes that",
        line2: "drum voice in the mix",
        color: C_RED,
      },
    ];

    for (let b = 0; b < boxes.length; b++) {
      const item = boxes[b]!;
      const bx = 42 + b * 220;
      ctx.fillStyle = "rgba(18, 25, 40, 0.9)";
      ctx.strokeStyle = item.color;
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.roundRect(bx, modY, 204, 66, 8);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = item.color;
      ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "left";
      ctx.fillText(item.title, bx + 12, modY + 23);

      ctx.fillStyle = C_TEXT;
      ctx.font = "12px system-ui, -apple-system, sans-serif";
      ctx.fillText(item.line1, bx + 12, modY + 42);
      ctx.fillText(item.line2, bx + 12, modY + 57);
    }

    if (localT > 12) {
      const kits = [
        { k: "Y", name: "Sub Boom" },
        { k: "U", name: "Club Punch" },
        { k: "I", name: "Studio Disco" },
        { k: "O", name: "Silver Matrix" },
        { k: "P", name: "Raw Warehouse" },
      ];
      // Slow kit highlight: 3.5 seconds per kit
      const activeKit = Math.floor(((localT - 12) * 0.28) % 5);
      for (let k = 0; k < kits.length; k++) {
        const kx = 68 + k * 122;
        const ky = 252;
        const on = k === activeKit;
        ctx.fillStyle = on ? "rgba(92, 208, 179, 0.25)" : "rgba(22, 32, 50, 0.85)";
        ctx.strokeStyle = on ? C_YELLOW : C_TEAL;
        ctx.lineWidth = on ? 2.2 : 1.4;
        ctx.beginPath();
        ctx.roundRect(kx, ky, 108, 54, 7);
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = C_TEXT;
        ctx.font = "bold 14px ui-monospace, SFMono-Regular, Menlo, monospace";
        ctx.textAlign = "center";
        ctx.fillText(`Key ${kits[k]!.k}`, kx + 54, ky + 22);
        ctx.fillStyle = on ? C_YELLOW : C_TEAL;
        ctx.font = "11.5px system-ui, sans-serif";
        ctx.fillText(kits[k]!.name, kx + 54, ky + 42);
      }
    }
    ctx.textAlign = "left";
  }

  /**
   * Chapter 3 (60..90s): Calm oscilloscope view of the synthesized analog drums.
   */
  private drawChapter3AnalogDsp(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("How the Synthesized Drum Voices Shape Your Sound", 24, 32);

    const panels = [
      {
        title: "Bass Drum (Kick)",
        subtitle: "Tune sets sub pitch · Decay sets boom length",
        color: C_BLUE,
        y0: 92,
      },
      {
        title: "Hi-Hats (Closed & Open)",
        subtitle: "6 metallic oscillators · Closed Hat chokes Open Hat",
        color: C_YELLOW,
        y0: 188,
      },
      {
        title: "Snare & Hand-Clap",
        subtitle: "Snappy adds wire rattle · 4 rapid clap bursts + room tail",
        color: C_RED,
        y0: 284,
      },
    ];

    const pulsePhase = (localT * 0.22) % 1;

    for (let p = 0; p < panels.length; p++) {
      const pan = panels[p]!;
      ctx.fillStyle = pan.color;
      ctx.font = 'bold 14px Georgia, "Times New Roman", serif';
      ctx.fillText(pan.title, 26, pan.y0 - 18);

      ctx.fillStyle = C_MUTED;
      ctx.font = "12px system-ui, -apple-system, sans-serif";
      ctx.fillText(pan.subtitle, 26, pan.y0 + 2);

      const xStart = 305;
      const xWidth = 385;
      ctx.strokeStyle = C_AXIS;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(xStart, pan.y0);
      ctx.lineTo(xStart + xWidth, pan.y0);
      ctx.stroke();

      ctx.strokeStyle = pan.color;
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (let ix = 0; ix <= xWidth; ix++) {
        const u = ix / xWidth;
        let sample = 0;
        if (p === 0) {
          const env = Math.exp(-u * 4.2);
          const instPhase = 2 * Math.PI * (12 * u - 2.8 * Math.exp(-u * 10));
          sample = env * Math.sin(instPhase - pulsePhase * Math.PI * 2);
        } else if (p === 1) {
          const env = Math.exp(-u * 7.5);
          const s =
            Math.sin(u * 85 + localT * 1.8) +
            Math.sin(u * 127 - localT * 1.4) +
            Math.sin(u * 159 + localT * 2.1);
          sample = (s / 3) * env;
        } else {
          const strikes = [0, 0.09, 0.19, 0.28];
          let env = 0;
          for (const st of strikes) {
            if (u >= st) env = Math.max(env, Math.exp(-(u - st) * 18));
          }
          if (u >= 0.28) env = Math.max(env, 0.45 * Math.exp(-(u - 0.28) * 3.8));
          const carrier = Math.sin(u * 160 + localT * 2.5) * Math.cos(u * 112);
          sample = env * carrier;
        }
        const py = pan.y0 - sample * 28;
        if (ix === 0) ctx.moveTo(xStart + ix, py);
        else ctx.lineTo(xStart + ix, py);
      }
      ctx.stroke();
    }
  }

  /**
   * Chapter 4 (90..120s): RMX-1000 Performance Keys & Pioneer Booth Sync.
   */
  private drawChapter4RmxAndSync(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("RMX-1000 Performance Keys  &  DJ Booth Tempo Sync", 24, 32);

    const startY = 52;
    const rowH = 54;
    for (let r = 0; r < MACBOOK_ROWS.length; r++) {
      const row = MACBOOK_ROWS[r]!;
      const offsetX = 28 + r * 14;
      for (let c = 0; c < row.length; c++) {
        const cap = row[c]!;
        const x = offsetX + c * 54;
        const y = startY + r * rowH;
        const highlightRmx =
          (localT < 10 && ["Backquote", "KeyQ", "KeyW", "KeyE", "KeyR"].includes(cap.code)) ||
          (localT >= 10 && localT < 20 && cap.code === "KeyR") ||
          (localT >= 20 &&
            ["KeyT", "BracketLeft", "BracketRight", "Semicolon", "Quote", "KeyL"].includes(cap.code)) ||
          this.activeCodes.has(cap.code);

        ctx.fillStyle = highlightRmx ? "rgba(247, 209, 84, 0.28)" : "rgba(20, 29, 46, 0.9)";
        ctx.strokeStyle = highlightRmx ? C_YELLOW : cap.color;
        ctx.lineWidth = highlightRmx ? 2.4 : 1.3;
        ctx.beginPath();
        ctx.roundRect(x, y, 48, 44, 6);
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = C_TEXT;
        ctx.font = "bold 12px ui-monospace, SFMono-Regular, Menlo, monospace";
        ctx.textAlign = "center";
        ctx.fillText(cap.label, x + 24, y + 17);

        ctx.fillStyle = highlightRmx ? C_YELLOW : cap.color;
        ctx.font = "9.5px system-ui, sans-serif";
        ctx.fillText(cap.sub, x + 24, y + 34);
      }
    }
    ctx.textAlign = "left";

    const spaceActive = this.activeCodes.has("Space");
    ctx.fillStyle = spaceActive ? "rgba(247, 209, 84, 0.28)" : "rgba(20, 29, 46, 0.9)";
    ctx.strokeStyle = spaceActive ? C_YELLOW : C_BLUE;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.roundRect(210, 276, 300, 40, 7);
    ctx.fill();
    ctx.stroke();

    ctx.fillStyle = C_TEXT;
    ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.textAlign = "center";
    ctx.fillText("SPACE  ·  Play / Stop Transport", 360, 301);
    ctx.textAlign = "left";
  }
}

export function howToModalMarkup(): string {
  const chapterButtons = CHAPTERS.map(
    (c) =>
      `<button type="button" class="howto-ch-btn" data-howto-chapter="${c.id}" aria-pressed="${c.id === 0}">${c.title}</button>`,
  ).join("");

  const keyboardLegend = MACBOOK_ROWS.map(
    (row, rIdx) =>
      `<div class="kb-row kb-row-${rIdx}">${row
        .map(
          (k) =>
            `<button type="button" class="kb-cap" data-key-code="${k.code}" data-key-desc="${k.label} → ${k.sub}" title="${k.label}: ${k.sub}"><kbd>${k.label}</kbd><span>${k.sub}</span></button>`,
        )
        .join("")}</div>`,
  ).join("");

  return `
  <div id="how-to-modal" class="howto-backdrop" role="dialog" aria-modal="true" aria-labelledby="howto-title" hidden>
    <div class="panel howto-modal">
      <div class="panel-head howto-head">
        <div>
          <span class="howto-badge">Visual Guide &amp; Quick Start · MacBook Air</span>
          <h2 id="howto-title">How to Use player5 &amp; Keyboard Shortcuts</h2>
        </div>
        <button id="howto-close" type="button" class="small" aria-label="Close How To guide">Close (Esc)</button>
      </div>

      <section class="howto-quickstart" id="howto-quickstart" aria-labelledby="howto-qs-title">
        <div class="panel-head">
          <h3 id="howto-qs-title">How to Get Started Quickly (Step by Step)</h3>
          <output id="howto-live-key" class="howto-live-key" aria-live="polite">Press any MacBook key to test it live</output>
        </div>
        <ol class="howto-steps">
          <li>
            <strong>1. Start the beat &amp; pick a groove:</strong>
            Press <kbd>Space</kbd> (or click <strong>Play</strong>) to start audio. Choose a style from <strong>Presets</strong> (<em>Nu-Disco</em>, <em>House</em>, or <em>Techno</em>) and swap the drum machine sound character anytime using <strong>Drum kit</strong> (or keys <kbd>Y</kbd> <kbd>U</kbd> <kbd>I</kbd> <kbd>O</kbd> <kbd>P</kbd>).
          </li>
          <li>
            <strong>2. Select or play a drum sound (<kbd>1</kbd>–<kbd>0</kbd>):</strong>
            Press number keys <kbd>1</kbd>–<kbd>0</kbd> (or click <strong>BD</strong>, <strong>SD</strong>, <strong>CP</strong>, <strong>CH</strong> on the left) to select a drum row. Hold <kbd>Shift</kbd> + <kbd>1</kbd>–<kbd>0</kbd> to play that drum live, or <kbd>⌥ Option</kbd> + <kbd>1</kbd>–<kbd>0</kbd> to mute/unmute it.
          </li>
          <li>
            <strong>3. Program the 16 steps (<kbd>A</kbd>–<kbd>K</kbd> &amp; <kbd>Z</kbd>–<kbd>,</kbd>):</strong>
            Your MacBook Home Row <kbd>A</kbd>–<kbd>K</kbd> controls <strong>Steps 1–8</strong>, and the Bottom Row <kbd>Z</kbd>–<kbd>,</kbd> controls <strong>Steps 9–16</strong>. Tap once for a <em>Hit</em>, twice for a louder <em>Accent</em>, and a third time to turn it <em>Off</em> (hold <kbd>Shift</kbd> + step key for a double-hit <em>Flam</em>).
          </li>
          <li>
            <strong>4. Shape the analog circuit &amp; swing:</strong>
            Use the <strong>Voice</strong> sliders (<em>Tune</em>, <em>Decay</em>, <em>Tone</em>, <em>Snappy</em>) below the grid to sculpt the selected drum, and raise <strong>Shuffle</strong> in the top bar for house/disco swing.
          </li>
          <li>
            <strong>5. Perform with RMX-1000 FX &amp; sync to DJ gear:</strong>
            Press <kbd>\`</kbd> to flip to the <strong>RMX-1000 Station</strong> for beat rolls, Isolator kills, and Scene FX build-ups—and hold <kbd>R</kbd> to pull the <strong>Release FX lever</strong> (Vinyl Brake / Echo Out). Use the <strong>Clock</strong> panel (<kbd>T</kbd> Tap, <strong>Bridge</strong>, or <strong>MIDI</strong>) plus <kbd>[</kbd> <kbd>]</kbd> and <kbd>;</kbd> <kbd>'</kbd> to lock phase with Pioneer CDJs.
          </li>
        </ol>
      </section>

      <div class="howto-chapters" role="group" aria-label="Video chapters">
        ${chapterButtons}
      </div>

      <div class="howto-stage">
        <canvas id="manim-canvas" width="720" height="340" aria-label="3Blue1Brown style visual explainer animation"></canvas>
        <p id="howto-caption" class="howto-caption" aria-live="polite"></p>
      </div>

      <div class="howto-transport">
        <button id="howto-play" type="button" class="toggle" aria-pressed="true">Pause</button>
        <input id="howto-scrub" type="range" min="0" max="120" step="0.1" value="0" aria-label="Video timeline" />
        <output id="howto-time" class="mono-out">0:00 / 2:00</output>
        <button id="howto-speed" type="button" class="small" title="Playback speed">1×</button>
        <button id="howto-sound" type="button" class="toggle small" aria-pressed="false" title="Toggle harmonic chapter chimes">Chimes: OFF</button>
      </div>

      <div class="howto-kb-section">
        <div class="panel-head">
          <h3>MacBook Air Interactive Key Map</h3>
          <span class="hint">Click any key below or press it on your keyboard</span>
        </div>
        <div class="kb-diagram" role="group" aria-label="MacBook Air keyboard shortcuts">
          ${keyboardLegend}
        </div>
      </div>
    </div>
  </div>`;
}
