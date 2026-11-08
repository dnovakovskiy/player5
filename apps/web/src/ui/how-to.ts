// 3Blue1Brown / Manim-style interactive video explainer and MacBook Air
// keyboard controller for player5. Renders a 60 FPS procedural vector-math
// animation on an HTML5 <canvas> with synchronized captions, chapter markers,
// timeline scrubbing, optional procedural Manim-style chimes, and live
// MacBook Air key-highlight feedback.

export interface Chapter {
  id: number;
  title: string;
  formula: string;
  startSec: number;
  endSec: number;
}

export const EXPLAINER_DURATION_SEC = 60;

export const CHAPTERS: readonly Chapter[] = [
  {
    id: 0,
    title: "1 · Phase Circle & 16-Step Home Row",
    formula: "θ(t) = 2π · (BPM / 60) · t   →   Steps 1–8: [A..K] · Steps 9–16: [Z..,]",
    startSec: 0,
    endSec: 15,
  },
  {
    id: 1,
    title: "2 · 10 Voices, Kits & Mute Matrix",
    formula: "Voices V₁..V₁₀: [1..0] · Live Trigger: Shift+[1..0] · Kits: [Y U I O P]",
    startSec: 15,
    endSec: 30,
  },
  {
    id: 2,
    title: "3 · Analog Drum Circuit Math",
    formula: "y(t) = e^(−t/τ) · sin(2π ∫ f(τ) dτ)  +  ∑ sgn(sin 2π fₖ t)",
    startSec: 30,
    endSec: 45,
  },
  {
    id: 3,
    title: "4 · RMX-1000 & Pioneer Booth Sync",
    formula: "Surface: [`] · X-Pad: [Q W E R] · Release Lever: Hold [R] · Nudge: [; ']",
    startSec: 45,
    endSec: 60,
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
    to: 5,
    text: "Think of a 4/4 bar not as a static row of boxes, but as a phase vector rotating around a unit circle at angular velocity ω = 2π · BPM / 240.",
  },
  {
    from: 5,
    to: 10,
    text: "Unrolling those 16 sixteenth-note phase angles onto your MacBook Air keyboard places Steps 1–8 on the home row (A S D F G H J K) and Steps 9–16 directly below (Z X C V B N M ,).",
  },
  {
    from: 10,
    to: 15,
    text: "Tapping any step key cycles Off → Hit (x) → Accent (X). Holding Shift while pressing a step key adds a pre-beat Flam grace note (−Δt) in a single stroke.",
  },
  {
    from: 15,
    to: 20,
    text: "The number row 1 through 0 maps directly to the 10 analog drum voices: Kick, Snare, Low/Mid/High Toms, Rimshot, Clap, Closed/Open Hats, and Cowbell.",
  },
  {
    from: 20,
    to: 25,
    text: "Hold Shift + 1..0 to finger-drum any voice live, or hold Option (⌥) + 1..0 to toggle its mute state Mₖ ∈ {0, 1} in the mix without moving your hands.",
  },
  {
    from: 25,
    to: 30,
    text: "Keys Y, U, I, O, P hot-swap the 5 analog drum machine circuit voicings—from deep booming sub-bass to crisp studio disco and raw warehouse techno—while preserving your pattern.",
  },
  {
    from: 30,
    to: 35,
    text: "Every drum sound is synthesized from first principles: the kick is a bridged-T resonator whose instantaneous frequency f(t) sweeps exponentially downward on each trigger.",
  },
  {
    from: 35,
    to: 40,
    text: "Hi-hats sum 6 inharmonic square-wave oscillators through a high-Q bandpass filter, while the hand-clap fires 4 sawtooth-scheduled noise bursts at 0, 10, 21, and 31 ms.",
  },
  {
    from: 40,
    to: 45,
    text: "Increasing Shuffle warps odd-numbered sixteenth steps later in phase by Δθ, creating the signature swung pocket of classic house and boogie.",
  },
  {
    from: 45,
    to: 50,
    text: "Press Backquote (`) to flip into the Pioneer RMX-1000 Remix Station. Keys Q, W, E, R trigger the 4 X-Pad drum pads, while 5–0 engage 1/1 to 1/32 beat-roll ribbons.",
  },
  {
    from: 50,
    to: 55,
    text: "Hold R at any time to pull the spring-loaded Release FX lever (Vinyl Brake, Echo Out, or Back Spin). Releasing R snaps cleanly back into the dry 16-step groove.",
  },
  {
    from: 55,
    to: 60,
    text: "In the booth, use [ and ] to trim BPM, ; and ' to nudge phase by ±1 ms, T to tap tempo, and Space to start or stop playback.",
  },
];

// Manim signature color palette
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
  private readonly formulaEl: HTMLElement;
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
    this.formulaEl = root.querySelector<HTMLElement>("#howto-formula")!;
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
      this.speed = this.speed === 1 ? 1.5 : this.speed === 1.5 ? 0.75 : 1;
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
        }, 350);
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
      // Optional soft harmonic chime on quarter-bar transitions
      const chimeIdx = Math.floor(this.currentSec * 2);
      if (this.soundOn && chimeIdx !== this.lastChimeStep) {
        this.lastChimeStep = chimeIdx;
        const scale = [261.6, 329.6, 392.0, 523.25, 659.25];
        const freq = scale[chimeIdx % scale.length]!;
        if (chimeIdx % 4 === 0) this.playChime(freq, 0.06);
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
    this.formulaEl.textContent = ch.formula;
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
    const H = 360;
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

    // 2. Dispatch by Chapter
    if (t < 15) {
      this.drawChapter1PhaseCircle(ctx, W, H, t);
    } else if (t < 30) {
      this.drawChapter2VoiceMatrix(ctx, W, H, t - 15);
    } else if (t < 45) {
      this.drawChapter3AnalogDsp(ctx, W, H, t - 30);
    } else {
      this.drawChapter4RmxAndSync(ctx, W, H, t - 45);
    }
  }

  /**
   * Chapter 1 (0..15s): The 16-Step Phase Circle θ(t) morphing onto the
   * MacBook Air Home Row (A..K) & Bottom Row (Z..,).
   */
  private drawChapter1PhaseCircle(
    ctx: CanvasRenderingContext2D,
    W: number,
    H: number,
    localT: number,
  ): void {
    const cx = 165;
    const cy = 175;
    const R = 96;
    const morph = smoothstep(4.2, 8.5, localT);

    // Title in Manim serif style
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Phase Circle  S¹  →  MacBook Air 16-Step Matrix", 24, 32);

    // Coordinate axes through circle center
    ctx.strokeStyle = C_AXIS;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(cx - R - 22, cy);
    ctx.lineTo(cx + R + 22, cy);
    ctx.moveTo(cx, cy - R - 22);
    ctx.lineTo(cx, cy + R + 22);
    ctx.stroke();

    // Unit circle
    ctx.strokeStyle = C_BLUE;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(cx, cy, R, 0, Math.PI * 2);
    ctx.stroke();

    // Rotating phase vector θ(t)
    const phase = (localT * 0.85) % 1;
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

    // 16 step nodes morphing from circle to the two 8-key rows on the right
    const stepKeys = ["A", "S", "D", "F", "G", "H", "J", "K", "Z", "X", "C", "V", "B", "N", "M", ","];
    const hitSteps = new Set([0, 4, 8, 12, 2, 6, 10, 14]);
    const accentSteps = new Set([0, 8]);

    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * Math.PI * 2 - Math.PI / 2;
      const sx = cx + Math.cos(a) * R;
      const sy = cy + Math.sin(a) * R;

      // Target keyboard position on right half of canvas
      const row = i < 8 ? 0 : 1;
      const col = i % 8;
      const tx = 345 + col * 44 + row * 12;
      const ty = 135 + row * 68;

      const x = sx + (tx - sx) * morph;
      const y = sy + (ty - sy) * morph;
      const isCurrent = i === activeStep;
      const isAccent = accentSteps.has(i);
      const isHit = hitSteps.has(i);

      // Connecting trajectory line during morph
      if (morph > 0.05 && morph < 0.98) {
        ctx.strokeStyle = "rgba(88, 196, 221, 0.22)";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(sx, sy);
        ctx.lineTo(x, y);
        ctx.stroke();
      }

      // Draw key box when morphed, or circle node
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

    // Row labels on right
    if (morph > 0.4) {
      ctx.fillStyle = C_GREEN;
      ctx.font = 'italic 14px Georgia, "Times New Roman", serif';
      ctx.fillText("Home Row (Steps 1–8):  A  S  D  F  G  H  J  K", 345, 98);
      ctx.fillText("Bottom Row (Steps 9–16):  Z  X  C  V  B  N  M  ,", 355, 250);
    }

    // Bottom mathematical callout
    if (localT > 9.5) {
      ctx.fillStyle = C_YELLOW;
      ctx.font = 'italic 15px Georgia, "Times New Roman", serif';
      ctx.fillText(
        "Tap key:  Off (-) → Hit (x) → Accent (X)     |     Hold Shift + key:  Toggle Flam (f / F)",
        64,
        328,
      );
    }
  }

  /**
   * Chapter 2 (15..30s): Number Row 1..0 → 10 Voices, Shift Audition,
   * Option Mute Matrix, and Y U I O P Analog Kits.
   */
  private drawChapter2VoiceMatrix(
    ctx: CanvasRenderingContext2D,
    W: number,
    H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Voice Selection Vector  Vₖ  &  Kit Parameter Basis  [Y U I O P]", 24, 32);

    const voices = ["BD", "SD", "LT", "MT", "HT", "RS", "CP", "CH", "OH", "CB"];
    const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    const scanIdx = Math.floor((localT * 1.8) % 10);

    // Draw top number row keys 1..0
    for (let i = 0; i < 10; i++) {
      const x = 52 + i * 64;
      const y = 86;
      const active = i === scanIdx || this.activeCodes.has(`Digit${keys[i]}`);

      ctx.fillStyle = active ? "rgba(88, 196, 221, 0.26)" : "rgba(22, 32, 50, 0.88)";
      ctx.strokeStyle = active ? C_YELLOW : C_BLUE;
      ctx.lineWidth = active ? 2.4 : 1.4;
      ctx.beginPath();
      ctx.roundRect(x, y, 52, 54, 7);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = C_TEXT;
      ctx.font = "bold 15px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(keys[i]!, x + 26, y + 22);

      ctx.fillStyle = active ? C_YELLOW : C_BLUE;
      ctx.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(voices[i]!, x + 26, y + 42);
    }

    // Draw modifier branches (Plain vs Shift vs Option)
    const modY = 178;
    const boxes = [
      {
        title: "Press  [1 .. 0]",
        desc: "Selects voice row Vₖ for step editing & knob shaping",
        color: C_BLUE,
      },
      {
        title: "Hold Shift + [1 .. 0]",
        desc: "Triggers live analog drum hit (finger-drumming / overdub)",
        color: C_YELLOW,
      },
      {
        title: "Hold ⌥ Option + [1 .. 0]",
        desc: "Toggles Mute Mₖ ∈ {0, 1} on that voice instantly",
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
      ctx.fillText(item.title, bx + 12, modY + 24);

      ctx.fillStyle = C_TEXT;
      ctx.font = "11.5px system-ui, -apple-system, sans-serif";
      ctx.fillText(item.desc.slice(0, 32), bx + 12, modY + 43);
      ctx.fillText(item.desc.slice(32), bx + 12, modY + 58);
    }

    // Draw Y U I O P Kit Voicing Basis at bottom
    if (localT > 6) {
      const kits = [
        { k: "Y", name: "Sub Boom" },
        { k: "U", name: "Club Punch" },
        { k: "I", name: "Studio Disco" },
        { k: "O", name: "Silver Matrix" },
        { k: "P", name: "Raw Warehouse" },
      ];
      const activeKit = Math.floor(((localT - 6) * 0.9) % 5);
      for (let k = 0; k < kits.length; k++) {
        const kx = 68 + k * 122;
        const ky = 272;
        const on = k === activeKit;
        ctx.fillStyle = on ? "rgba(92, 208, 179, 0.25)" : "rgba(22, 32, 50, 0.85)";
        ctx.strokeStyle = on ? C_YELLOW : C_TEAL;
        ctx.lineWidth = on ? 2.2 : 1.4;
        ctx.beginPath();
        ctx.roundRect(kx, ky, 108, 52, 7);
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = C_TEXT;
        ctx.font = "bold 14px ui-monospace, SFMono-Regular, Menlo, monospace";
        ctx.textAlign = "center";
        ctx.fillText(`Key ${kits[k]!.k}`, kx + 54, ky + 22);
        ctx.fillStyle = on ? C_YELLOW : C_TEAL;
        ctx.font = "11px system-ui, sans-serif";
        ctx.fillText(kits[k]!.name, kx + 54, ky + 41);
      }
    }
    ctx.textAlign = "left";
  }

  /**
   * Chapter 3 (30..45s): Real-time oscilloscope plots of the 3 analog circuits:
   * Bridged-T Kick, 6-Oscillator Metallic Hats, and 4-Burst Flutter Clap.
   */
  private drawChapter3AnalogDsp(
    ctx: CanvasRenderingContext2D,
    W: number,
    H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Synthesized Analog Drum Circuits  (Zero Samples · Pure Math)", 24, 32);

    const panels = [
      {
        title: "Bridged-T Sub Kick",
        eq: "y(t) = e^(−t/τ) · sin(2π ∫ f(τ) dτ)",
        color: C_BLUE,
        y0: 95,
      },
      {
        title: "6-Oscillator Metallic Hat Cluster",
        eq: "Σₖ sgn(sin 2π fₖ t)  →  BPF 8.2 kHz",
        color: C_YELLOW,
        y0: 195,
      },
      {
        title: "4-Burst Sawtooth Hand-Clap",
        eq: "E(t) = Σᵢ₌₀³ e^(−(t − tᵢ)/τ)  (0, 10, 21, 31 ms)",
        color: C_RED,
        y0: 295,
      },
    ];

    const pulsePhase = (localT * 0.9) % 1;

    for (let p = 0; p < panels.length; p++) {
      const pan = panels[p]!;
      ctx.fillStyle = pan.color;
      ctx.font = 'bold 13px Georgia, "Times New Roman", serif';
      ctx.fillText(pan.title, 28, pan.y0 - 22);

      ctx.fillStyle = C_MUTED;
      ctx.font = 'italic 12px Georgia, "Times New Roman", serif';
      ctx.fillText(pan.eq, 28, pan.y0 - 4);

      // Waveform axis
      const xStart = 285;
      const xWidth = 405;
      ctx.strokeStyle = C_AXIS;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(xStart, pan.y0);
      ctx.lineTo(xStart + xWidth, pan.y0);
      ctx.stroke();

      // Plot waveform
      ctx.strokeStyle = pan.color;
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (let ix = 0; ix <= xWidth; ix++) {
        const u = ix / xWidth;
        let sample = 0;
        if (p === 0) {
          // Swept bridged-T sine wave
          const env = Math.exp(-u * 4.2);
          const instPhase = 2 * Math.PI * (14 * u - 3.2 * Math.exp(-u * 12));
          sample = env * Math.sin(instPhase - pulsePhase * Math.PI * 2);
        } else if (p === 1) {
          // 6-oscillator metallic cluster
          const env = Math.exp(-u * 7.5);
          const s =
            Math.sin(u * 95 + localT * 8) +
            Math.sin(u * 141 - localT * 6) +
            Math.sin(u * 173 + localT * 11) +
            Math.sin(u * 244);
          sample = (s / 4) * env;
        } else {
          // 4-burst sawtooth flutter clap envelope
          const strikes = [0, 0.09, 0.19, 0.28];
          let env = 0;
          for (const st of strikes) {
            if (u >= st) env = Math.max(env, Math.exp(-(u - st) * 18));
          }
          if (u >= 0.28) env = Math.max(env, 0.45 * Math.exp(-(u - 0.28) * 3.8));
          const carrier = Math.sin(u * 210 + localT * 15) * Math.cos(u * 137);
          sample = env * carrier;
        }
        const py = pan.y0 - sample * 30;
        if (ix === 0) ctx.moveTo(xStart + ix, py);
        else ctx.lineTo(xStart + ix, py);
      }
      ctx.stroke();
    }
  }

  /**
   * Chapter 4 (45..60s): RMX-1000 Performance Keys & Pioneer Pro DJ Link Sync.
   */
  private drawChapter4RmxAndSync(
    ctx: CanvasRenderingContext2D,
    W: number,
    H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("RMX-1000 Performance Surface  &  Pioneer Pro DJ Link Phase Lock", 24, 32);

    // Draw interactive MacBook Air keyboard map
    const startY = 58;
    const rowH = 56;
    for (let r = 0; r < MACBOOK_ROWS.length; r++) {
      const row = MACBOOK_ROWS[r]!;
      const offsetX = 28 + r * 14;
      for (let c = 0; c < row.length; c++) {
        const cap = row[c]!;
        const x = offsetX + c * 54;
        const y = startY + r * rowH;
        const highlightRmx =
          (localT < 5 && ["Backquote", "KeyQ", "KeyW", "KeyE", "KeyR"].includes(cap.code)) ||
          (localT >= 5 && localT < 10 && cap.code === "KeyR") ||
          (localT >= 10 && ["KeyT", "BracketLeft", "BracketRight", "Semicolon", "Quote", "KeyL"].includes(cap.code)) ||
          this.activeCodes.has(cap.code);

        ctx.fillStyle = highlightRmx ? "rgba(247, 209, 84, 0.28)" : "rgba(20, 29, 46, 0.9)";
        ctx.strokeStyle = highlightRmx ? C_YELLOW : cap.color;
        ctx.lineWidth = highlightRmx ? 2.4 : 1.3;
        ctx.beginPath();
        ctx.roundRect(x, y, 48, 46, 6);
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = C_TEXT;
        ctx.font = "bold 12px ui-monospace, SFMono-Regular, Menlo, monospace";
        ctx.textAlign = "center";
        ctx.fillText(cap.label, x + 24, y + 18);

        ctx.fillStyle = highlightRmx ? C_YELLOW : cap.color;
        ctx.font = "9.5px system-ui, sans-serif";
        ctx.fillText(cap.sub, x + 24, y + 36);
      }
    }
    ctx.textAlign = "left";

    // Spacebar at bottom
    const spaceActive = this.activeCodes.has("Space");
    ctx.fillStyle = spaceActive ? "rgba(247, 209, 84, 0.28)" : "rgba(20, 29, 46, 0.9)";
    ctx.strokeStyle = spaceActive ? C_YELLOW : C_BLUE;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.roundRect(210, 290, 300, 42, 7);
    ctx.fill();
    ctx.stroke();

    ctx.fillStyle = C_TEXT;
    ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.textAlign = "center";
    ctx.fillText("SPACE  ·  Play / Stop Transport", 360, 316);
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
          <span class="howto-badge">3Blue1Brown Visual Guide · MacBook Air</span>
          <h2 id="howto-title">Geometry of the 16-Step Sequencer &amp; Keyboard Shortcuts</h2>
        </div>
        <button id="howto-close" type="button" class="small" aria-label="Close How To guide">Close (Esc)</button>
      </div>

      <div class="howto-chapters" role="group" aria-label="Video chapters">
        ${chapterButtons}
      </div>

      <div class="howto-stage">
        <canvas id="manim-canvas" width="720" height="360" aria-label="3Blue1Brown style mathematical animation"></canvas>
        <div class="howto-formula-bar">
          <output id="howto-formula" class="howto-formula">θ(t) = 2π · (BPM / 60) · t</output>
          <output id="howto-live-key" class="howto-live-key" aria-live="polite">Press any MacBook key to test it live</output>
        </div>
        <p id="howto-caption" class="howto-caption" aria-live="polite"></p>
      </div>

      <div class="howto-transport">
        <button id="howto-play" type="button" class="toggle" aria-pressed="true">Pause</button>
        <input id="howto-scrub" type="range" min="0" max="60" step="0.1" value="0" aria-label="Video timeline" />
        <output id="howto-time" class="mono-out">0:00 / 1:00</output>
        <button id="howto-speed" type="button" class="small" title="Playback speed">1×</button>
        <button id="howto-sound" type="button" class="toggle small" aria-pressed="false" title="Toggle harmonic chapter chimes">Chimes: OFF</button>
      </div>

      <div class="howto-kb-section">
        <div class="panel-head">
          <h3>MacBook Air Interactive Key Map</h3>
          <span class="hint">Hold <strong>Shift + 1..0</strong> to finger-drum · <strong>⌥ Option + 1..0</strong> to mute · <strong>Shift + Step</strong> for Flam</span>
        </div>
        <div class="kb-diagram" role="group" aria-label="MacBook Air keyboard shortcuts">
          ${keyboardLegend}
        </div>
      </div>
    </div>
  </div>`;
}
