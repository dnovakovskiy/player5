// 3Blue1Brown-style interactive visual explainer, mode-specific step-by-step
// quick-start guides (Step Sequencer vs RMX-1000 Station), and MacBook Air
// keyboard map for player5.

export type GuideMode = "sequencer" | "rmx";

export interface Chapter {
  id: number;
  title: string;
  startSec: number;
  endSec: number;
}

export const EXPLAINER_DURATION_SEC = 120;

export const SEQ_CHAPTERS: readonly Chapter[] = [
  {
    id: 0,
    title: "1 · 16-Step Circle & Home Row",
    startSec: 0,
    endSec: 30,
  },
  {
    id: 1,
    title: "2 · 10 Voices & TR Drum Kits",
    startSec: 30,
    endSec: 60,
  },
  {
    id: 2,
    title: "3 · TR-8O8 / 9O9 / CR-78 Circuits",
    startSec: 60,
    endSec: 90,
  },
  {
    id: 3,
    title: "4 · Swing, Presets & Booth Sync",
    startSec: 90,
    endSec: 120,
  },
];

export const RMX_CHAPTERS: readonly Chapter[] = [
  {
    id: 0,
    title: "1 · RMX-1000 4-Section Signal Flow",
    startSec: 0,
    endSec: 30,
  },
  {
    id: 1,
    title: "2 · X-Pad Ribbon Rolls & Overdub",
    startSec: 30,
    endSec: 60,
  },
  {
    id: 2,
    title: "3 · Isolator Kills & Scene FX",
    startSec: 60,
    endSec: 90,
  },
  {
    id: 3,
    title: "4 · Release FX Lever Drop (Hold R)",
    startSec: 90,
    endSec: 120,
  },
];

export const CHAPTERS = SEQ_CHAPTERS;

interface Caption {
  from: number;
  to: number;
  text: string;
}

const SEQ_CAPTIONS: readonly Caption[] = [
  {
    from: 0,
    to: 10,
    text: "Step Sequencer: A 16-step pattern is one musical bar of 4 beats, divided into 16 sixteenth-note steps that repeat in a continuous loop.",
  },
  {
    from: 10,
    to: 20,
    text: "Unrolling that circle onto your MacBook Air keyboard puts Steps 1–8 on the Home Row (A S D F G H J K) and Steps 9–16 on the Bottom Row (Z X C V B N M ,).",
  },
  {
    from: 20,
    to: 30,
    text: "Tap any step key once for a normal Hit, twice for a louder Accent, and a third time to turn it Off. Hold Shift + a step key (or press E) to toggle a double-hit Flam.",
  },
  {
    from: 30,
    to: 40,
    text: "Number keys 1 through 0 select which of the 10 drum voices you are editing: Kick, Snare, Low/Mid/High Toms, Rimshot, Clap, Closed/Open Hats, and Cowbell.",
  },
  {
    from: 40,
    to: 50,
    text: "Hold Shift + 1..0 to play any drum sound live with your fingers, or hold Option (⌥) + 1..0 to mute and unmute that drum row in the mix.",
  },
  {
    from: 50,
    to: 60,
    text: "Use the Drum kit menu (or press Y, U, I, O, P) to switch between classic TR-8O8, TR-9O9, TR-7O7, TR-6O6, TR-72x Latin, and CR-78 drum machine kits.",
  },
  {
    from: 60,
    to: 70,
    text: "Inside the TR circuits: TR-8O8 uses a deep Bridged-T sub-bass resonator, while TR-9O9 adds a 2-stage triangle pitch-drop transient for club punch.",
  },
  {
    from: 70,
    to: 80,
    text: "Closed and Open Hi-Hats blend 6 metallic oscillators—and just like a real hi-hat pedal, triggering a Closed Hat automatically chokes a ringing Open Hat.",
  },
  {
    from: 80,
    to: 90,
    text: "TR-7O7 adds crisp studio PCM transients, TR-72x adds Bongo/Conga/Agogo percussion, and CR-78 brings warm vintage woodblock clave and metallic tambourine.",
  },
  {
    from: 90,
    to: 100,
    text: "Raising Shuffle delays every even-numbered step (Steps 2, 4, 6, 8...) to create authentic House and Nu-Disco swing.",
  },
  {
    from: 100,
    to: 110,
    text: "Press Q and W in the Step Sequencer to audition the 17 built-in Nu-Disco, House, Techno, and Core Booth preset patterns.",
  },
  {
    from: 110,
    to: 120,
    text: "Lock your Step Sequencer to a DJ set with Space (Play/Stop), T (Tap tempo), [ and ] (BPM), and ; or ' (±1 ms phase nudge).",
  },
];

const RMX_CAPTIONS: readonly Caption[] = [
  {
    from: 0,
    to: 10,
    text: "RMX-1000 Station: All audio flows through 4 live performance stages: X-Pad Sampler → 3-Band Isolator FX → Scene FX → Release FX Lever.",
  },
  {
    from: 10,
    to: 20,
    text: "Press Backquote (`) at any time to flip between the 16-Step Sequencer and the RMX-1000 Remix Station without interrupting audio playback.",
  },
  {
    from: 20,
    to: 30,
    text: "The top readout bar shows your active Scene FX % and X-Pad Beat Roll subdivision in real time so you always know what effect is engaged.",
  },
  {
    from: 30,
    to: 40,
    text: "X-Pad FX: Press Q, W, E (or 1, 2, 3, 4) to trigger the 4 X-Pad drum pads. Toggle Bank A (BD/SD/CP/CH) and Bank B (LT/MT/HT/CB) for 8 total voices.",
  },
  {
    from: 40,
    to: 50,
    text: "Touch Roll Ribbon: Hold keys 5, 6, 7, 8, 9, or 0 (or drag across the ribbon) to repeat the selected X-Pad drum at 1/1, 1/2, 1/4, 1/8, 1/16, or 1/32.",
  },
  {
    from: 50,
    to: 60,
    text: "Turn on Latch Roll to keep a beat roll running hands-free, use the Pitch slider (-12 to +12 semitones), or turn on Overdub to record hits into the pattern.",
  },
  {
    from: 60,
    to: 70,
    text: "Isolator FX: Sculpt Low, Mid, and High frequency bands from -48 dB (total kill) to +6 dB, or punch the KILL LOW / MID / HIGH buttons for instant drops.",
  },
  {
    from: 70,
    to: 80,
    text: "Switch the Isolator between ISOLATOR, CUT/ADD, TRANS/ROLL (16th-note rhythmic tremor), and GATE/DRIVE (analog saturation + noise gate).",
  },
  {
    from: 80,
    to: 90,
    text: "Scene FX: Combine 5 BUILD UP effects (BPF Echo, Echo, Noise, Spiral Up, Reverb Up) and 5 BREAK DOWN effects (HPF/LPF/Crush Echo, Spiral/Reverb Down).",
  },
  {
    from: 90,
    to: 100,
    text: "Release FX Lever: Hold R (or hold the Pull Release Lever button) at the peak of a build-up to engage Vinyl Brake, Echo Out, or Back Spin.",
  },
  {
    from: 100,
    to: 110,
    text: "While holding R, the dry drum mix cuts out as the turntable brake or spin-back rings out across the club speakers.",
  },
  {
    from: 110,
    to: 120,
    text: "The moment you let go of R, the spring-loaded lever snaps back, automatically resets Scene FX to 0%, and drops the clean beat right on the One!",
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
  seqSub: string;
  rmxSub: string;
  seqColor: string;
  rmxColor: string;
}

const MACBOOK_ROWS: readonly (readonly KeyCap[])[] = [
  [
    { code: "Backquote", label: "`", seqSub: "To RMX", rmxSub: "To Seq", seqColor: C_PURPLE, rmxColor: C_PURPLE },
    { code: "Digit1", label: "1", seqSub: "BD Kick", rmxSub: "X-Pad 1", seqColor: C_BLUE, rmxColor: C_BLUE },
    { code: "Digit2", label: "2", seqSub: "SD Snare", rmxSub: "X-Pad 2", seqColor: C_BLUE, rmxColor: C_BLUE },
    { code: "Digit3", label: "3", seqSub: "LT Tom", rmxSub: "X-Pad 3", seqColor: C_BLUE, rmxColor: C_BLUE },
    { code: "Digit4", label: "4", seqSub: "MT Tom", rmxSub: "X-Pad 4", seqColor: C_BLUE, rmxColor: C_BLUE },
    { code: "Digit5", label: "5", seqSub: "HT Tom", rmxSub: "Roll 1/1", seqColor: C_BLUE, rmxColor: C_YELLOW },
    { code: "Digit6", label: "6", seqSub: "RS Rim", rmxSub: "Roll 1/2", seqColor: C_BLUE, rmxColor: C_YELLOW },
    { code: "Digit7", label: "7", seqSub: "CP Clap", rmxSub: "Roll 1/4", seqColor: C_BLUE, rmxColor: C_YELLOW },
    { code: "Digit8", label: "8", seqSub: "CH Hat", rmxSub: "Roll 1/8", seqColor: C_BLUE, rmxColor: C_YELLOW },
    { code: "Digit9", label: "9", seqSub: "OH Hat", rmxSub: "Roll 1/16", seqColor: C_BLUE, rmxColor: C_YELLOW },
    { code: "Digit0", label: "0", seqSub: "CB Bell", rmxSub: "Roll 1/32", seqColor: C_BLUE, rmxColor: C_YELLOW },
    { code: "Backspace", label: "⌫", seqSub: "Clear Row", rmxSub: "Clear Pad", seqColor: C_RED, rmxColor: C_RED },
  ],
  [
    { code: "KeyQ", label: "Q", seqSub: "Prev Pat", rmxSub: "X-Pad 1", seqColor: C_PURPLE, rmxColor: C_BLUE },
    { code: "KeyW", label: "W", seqSub: "Next Pat", rmxSub: "X-Pad 2", seqColor: C_PURPLE, rmxColor: C_BLUE },
    { code: "KeyE", label: "E", seqSub: "Flam Edit", rmxSub: "X-Pad 3", seqColor: C_PURPLE, rmxColor: C_BLUE },
    { code: "KeyR", label: "R", seqSub: "Release FX", rmxSub: "HOLD LEVER", seqColor: C_RED, rmxColor: C_RED },
    { code: "KeyT", label: "T", seqSub: "Tap BPM", rmxSub: "Tap BPM", seqColor: C_YELLOW, rmxColor: C_YELLOW },
    { code: "KeyY", label: "Y", seqSub: "TR-8O8", rmxSub: "TR-8O8", seqColor: C_TEAL, rmxColor: C_TEAL },
    { code: "KeyU", label: "U", seqSub: "TR-9O9", rmxSub: "TR-9O9", seqColor: C_TEAL, rmxColor: C_TEAL },
    { code: "KeyI", label: "I", seqSub: "TR-7O7", rmxSub: "TR-7O7", seqColor: C_TEAL, rmxColor: C_TEAL },
    { code: "KeyO", label: "O", seqSub: "TR-6O6", rmxSub: "TR-6O6", seqColor: C_TEAL, rmxColor: C_TEAL },
    { code: "KeyP", label: "P", seqSub: "9O9 Drive", rmxSub: "9O9 Drive", seqColor: C_TEAL, rmxColor: C_TEAL },
    { code: "BracketLeft", label: "[", seqSub: "BPM −", rmxSub: "BPM −", seqColor: C_YELLOW, rmxColor: C_YELLOW },
    { code: "BracketRight", label: "]", seqSub: "BPM +", rmxSub: "BPM +", seqColor: C_YELLOW, rmxColor: C_YELLOW },
  ],
  [
    { code: "KeyA", label: "A", seqSub: "Step 1", rmxSub: "Step 1", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyS", label: "S", seqSub: "Step 2", rmxSub: "Step 2", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyD", label: "D", seqSub: "Step 3", rmxSub: "Step 3", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyF", label: "F", seqSub: "Step 4", rmxSub: "Step 4", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyG", label: "G", seqSub: "Step 5", rmxSub: "Step 5", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyH", label: "H", seqSub: "Step 6", rmxSub: "Step 6", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyJ", label: "J", seqSub: "Step 7", rmxSub: "Step 7", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyK", label: "K", seqSub: "Step 8", rmxSub: "Step 8", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyL", label: "L", seqSub: "Re-sync", rmxSub: "Re-sync", seqColor: C_YELLOW, rmxColor: C_YELLOW },
    { code: "Semicolon", label: ";", seqSub: "Nudge −", rmxSub: "Nudge −", seqColor: C_YELLOW, rmxColor: C_YELLOW },
    { code: "Quote", label: "'", seqSub: "Nudge +", rmxSub: "Nudge +", seqColor: C_YELLOW, rmxColor: C_YELLOW },
  ],
  [
    { code: "KeyZ", label: "Z", seqSub: "Step 9", rmxSub: "Step 9", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyX", label: "X", seqSub: "Step 10", rmxSub: "Step 10", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyC", label: "C", seqSub: "Step 11", rmxSub: "Step 11", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyV", label: "V", seqSub: "Step 12", rmxSub: "Step 12", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyB", label: "B", seqSub: "Step 13", rmxSub: "Step 13", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyN", label: "N", seqSub: "Step 14", rmxSub: "Step 14", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "KeyM", label: "M", seqSub: "Step 15", rmxSub: "Step 15", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "Comma", label: ",", seqSub: "Step 16", rmxSub: "Step 16", seqColor: C_GREEN, rmxColor: C_GREEN },
    { code: "Period", label: ".", seqSub: "Mute Row", rmxSub: "Mute Pad", seqColor: C_RED, rmxColor: C_RED },
    { code: "Slash", label: "?", seqSub: "How To", rmxSub: "How To", seqColor: C_YELLOW, rmxColor: C_YELLOW },
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
  private readonly modeSeqBtn: HTMLButtonElement;
  private readonly modeRmxBtn: HTMLButtonElement;
  private readonly qsSeqEl: HTMLElement;
  private readonly qsRmxEl: HTMLElement;
  private readonly kbHeadingEl: HTMLElement;

  private openState = false;
  private guideMode: GuideMode = "sequencer";
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
    this.modeSeqBtn = root.querySelector<HTMLButtonElement>("#howto-mode-seq")!;
    this.modeRmxBtn = root.querySelector<HTMLButtonElement>("#howto-mode-rmx")!;
    this.qsSeqEl = root.querySelector<HTMLElement>("#howto-qs-seq")!;
    this.qsRmxEl = root.querySelector<HTMLElement>("#howto-qs-rmx")!;
    this.kbHeadingEl = root.querySelector<HTMLElement>("#howto-kb-title")!;

    this.bind(root);
    this.syncGuideModeDom();
    this.renderUi();
  }

  get isOpen(): boolean {
    return this.openState;
  }

  get mode(): GuideMode {
    return this.guideMode;
  }

  setGuideMode(mode: GuideMode): void {
    if (this.guideMode === mode) return;
    this.guideMode = mode;
    this.currentSec = 0;
    this.syncGuideModeDom();
    this.renderUi();
    if (this.openState) this.drawFrame();
  }

  open(mode?: GuideMode): void {
    if (mode) {
      this.guideMode = mode;
      this.syncGuideModeDom();
    }
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

  toggle(mode?: GuideMode): void {
    if (this.openState) this.close();
    else this.open(mode);
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

  private activeChapters(): readonly Chapter[] {
    return this.guideMode === "rmx" ? RMX_CHAPTERS : SEQ_CHAPTERS;
  }

  private activeCaptions(): readonly Caption[] {
    return this.guideMode === "rmx" ? RMX_CAPTIONS : SEQ_CAPTIONS;
  }

  private syncGuideModeDom(): void {
    const isRmx = this.guideMode === "rmx";
    this.modal.dataset.guideMode = this.guideMode;
    this.modeSeqBtn.setAttribute("aria-pressed", String(!isRmx));
    this.modeRmxBtn.setAttribute("aria-pressed", String(isRmx));
    this.qsSeqEl.hidden = isRmx;
    this.qsRmxEl.hidden = !isRmx;

    const chapters = this.activeChapters();
    for (const [idx, btn] of this.chapterBtns.entries()) {
      const ch = chapters[idx];
      if (ch) btn.textContent = ch.title;
    }

    this.kbHeadingEl.textContent = isRmx
      ? "MacBook Air Key Map · RMX-1000 Station Mode"
      : "MacBook Air Key Map · Step Sequencer Mode";

    for (const cap of this.modal.querySelectorAll<HTMLElement>("[data-key-code]")) {
      const subText = (isRmx ? cap.dataset.rmxSub : cap.dataset.seqSub) ?? "";
      const label = cap.dataset.keyLabel ?? "";
      cap.dataset.keyDesc = `${label} → ${subText}`;
      cap.title = `${label}: ${subText}`;
      const span = cap.querySelector("span");
      if (span) span.textContent = subText;
    }
  }

  private bind(root: HTMLElement): void {
    root.querySelector<HTMLButtonElement>("#how-to-btn")?.addEventListener("click", () => {
      const currentSurface = root.dataset.surface === "rmx" ? "rmx" : "sequencer";
      this.toggle(currentSurface);
    });
    root.querySelector<HTMLButtonElement>("#howto-close")?.addEventListener("click", () => this.close());
    this.modal.addEventListener("click", (e) => {
      if (e.target === this.modal) this.close();
    });

    this.modeSeqBtn.addEventListener("click", () => this.setGuideMode("sequencer"));
    this.modeRmxBtn.addEventListener("click", () => this.setGuideMode("rmx"));

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
        const ch = this.activeChapters()[idx];
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

    const chapters = this.activeChapters();
    const ch =
      chapters.find((c) => this.currentSec >= c.startSec && this.currentSec < c.endSec) ?? chapters[0]!;
    for (const [idx, btn] of this.chapterBtns.entries()) {
      btn.setAttribute("aria-pressed", String(idx === ch.id));
    }

    const captions = this.activeCaptions();
    const cap =
      captions.find((c) => this.currentSec >= c.from && this.currentSec < c.to) ??
      captions[captions.length - 1]!;
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

    // 2. Dispatch by GuideMode ("sequencer" vs "rmx") and Chapter (30s each)
    if (this.guideMode === "rmx") {
      if (t < 30) {
        this.drawRmxChapter1SignalFlow(ctx, W, H, t);
      } else if (t < 60) {
        this.drawRmxChapter2XPadRibbon(ctx, W, H, t - 30);
      } else if (t < 90) {
        this.drawRmxChapter3IsolatorAndScene(ctx, W, H, t - 60);
      } else {
        this.drawRmxChapter4ReleaseLever(ctx, W, H, t - 90);
      }
    } else {
      if (t < 30) {
        this.drawSeqChapter1PhaseCircle(ctx, W, H, t);
      } else if (t < 60) {
        this.drawSeqChapter2VoiceMatrix(ctx, W, H, t - 30);
      } else if (t < 90) {
        this.drawSeqChapter3TrCircuits(ctx, W, H, t - 60);
      } else {
        this.drawSeqChapter4SwingAndSync(ctx, W, H, t - 90);
      }
    }
  }

  /**
   * STEP SEQUENCER Chapter 1 (0..30s): The 16-Step Bar Circle gently unrolling
   * onto the MacBook Air Home Row (A..K) & Bottom Row (Z..,).
   */
  private drawSeqChapter1PhaseCircle(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    const cx = 165;
    const cy = 170;
    const R = 94;
    const morph = smoothstep(8, 17, localT);

    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Step Sequencer · 16 Steps in One Bar  →  Home & Bottom Rows", 24, 32);

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
        "Tap key:  Off → Hit → Accent     |     Hold Shift + key (or E):  Add Flam",
        82,
        312,
      );
    }
  }

  /**
   * STEP SEQUENCER Chapter 2 (30..60s): Number Row 1..0 → 10 Voices,
   * Shift Audition, Option Mute, and Classic TR Drum Machine Kits.
   */
  private drawSeqChapter2VoiceMatrix(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Step Sequencer · [1 – 0] Drum Voices  &  7 Classic TR Drum Kits", 24, 32);

    const voices = ["BD", "SD", "LT", "MT", "HT", "RS", "CP", "CH", "OH", "CB"];
    const names = ["Kick", "Snare", "LoTom", "MidTom", "HiTom", "Rim", "Clap", "ClHat", "OpHat", "Bell"];
    const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    const scanIdx = Math.floor((localT * 0.45) % 10);

    for (let i = 0; i < 10; i++) {
      const x = 44 + i * 64;
      const y = 64;
      const active = i === scanIdx || this.activeCodes.has(`Digit${keys[i]}`);

      ctx.fillStyle = active ? "rgba(88, 196, 221, 0.26)" : "rgba(22, 32, 50, 0.88)";
      ctx.strokeStyle = active ? C_YELLOW : C_BLUE;
      ctx.lineWidth = active ? 2.4 : 1.4;
      ctx.beginPath();
      ctx.roundRect(x, y, 54, 60, 7);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = C_TEXT;
      ctx.font = "bold 15px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(keys[i]!, x + 27, y + 20);

      ctx.fillStyle = active ? C_YELLOW : C_BLUE;
      ctx.font = "bold 11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(voices[i]!, x + 27, y + 37);

      ctx.fillStyle = C_MUTED;
      ctx.font = "9.5px system-ui, sans-serif";
      ctx.fillText(names[i]!, x + 27, y + 52);
    }

    const modY = 144;
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
        line2: "(finger-drumming / audition)",
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
      ctx.roundRect(bx, modY, 204, 62, 8);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = item.color;
      ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "left";
      ctx.fillText(item.title, bx + 12, modY + 21);

      ctx.fillStyle = C_TEXT;
      ctx.font = "12px system-ui, -apple-system, sans-serif";
      ctx.fillText(item.line1, bx + 12, modY + 39);
      ctx.fillText(item.line2, bx + 12, modY + 54);
    }

    const kits = [
      { k: "Y", model: "TR-8O8", sub: "Sub Boom" },
      { k: "U", model: "TR-9O9", sub: "Club Punch" },
      { k: "I", model: "TR-7O7", sub: "Studio PCM" },
      { k: "O", model: "TR-6O6", sub: "Acid Silver" },
      { k: "P", model: "TR-9O9+", sub: "Overdrive" },
      { k: "Menu", model: "TR-72x", sub: "Latin Perc" },
      { k: "Menu", model: "CR-78", sub: "Vintage" },
    ];
    const activeKit = Math.floor((localT * 0.32) % kits.length);
    for (let k = 0; k < kits.length; k++) {
      const kx = 28 + k * 96;
      const ky = 236;
      const on = k === activeKit;
      ctx.fillStyle = on ? "rgba(92, 208, 179, 0.25)" : "rgba(22, 32, 50, 0.88)";
      ctx.strokeStyle = on ? C_YELLOW : C_TEAL;
      ctx.lineWidth = on ? 2.2 : 1.4;
      ctx.beginPath();
      ctx.roundRect(kx, ky, 88, 72, 7);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = on ? C_YELLOW : C_TEAL;
      ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(kits[k]!.model, kx + 44, ky + 22);

      ctx.fillStyle = C_TEXT;
      ctx.font = "11px system-ui, sans-serif";
      ctx.fillText(kits[k]!.sub, kx + 44, ky + 41);

      ctx.fillStyle = C_MUTED;
      ctx.font = "10px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(`Key: ${kits[k]!.k}`, kx + 44, ky + 59);
    }
    ctx.textAlign = "left";
  }

  /**
   * STEP SEQUENCER Chapter 3 (60..90s): Calm oscilloscope view of the
   * TR-8O8, TR-9O9, TR-7O7, and CR-78 drum synthesis circuits.
   */
  private drawSeqChapter3TrCircuits(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Step Sequencer · Authentic TR Drum Machine Voice Circuits", 24, 32);

    const panels = [
      {
        title: "TR-8O8 Sub Kick vs TR-9O9 Club Kick",
        subtitle: "8O8 long Bridged-T sub boom · 9O9 2-stage pitch click punch",
        color: C_BLUE,
        y0: 92,
      },
      {
        title: "6-Oscillator Metallic Hats & Choke",
        subtitle: "6 inharmonic square waves · Closed Hat chokes Open Hat",
        color: C_YELLOW,
        y0: 188,
      },
      {
        title: "TR-7O7 PCM Snare · TR-72x Latin · CR-78 Wood",
        subtitle: "Gated snare snap · Bongo/Conga/Agogo · CR-78 woodblock &guiro",
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

      const xStart = 325;
      const xWidth = 365;
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
   * STEP SEQUENCER Chapter 4 (90..120s): Swing Timing, Genre Presets (Q/W)
   * & Pioneer Pro DJ Link / MIDI Clock Booth Sync.
   */
  private drawSeqChapter4SwingAndSync(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("Step Sequencer · House/Disco Shuffle, Presets & DJ Booth Sync", 24, 32);

    // Top visual: Straight vs Shuffled 16th-note grid
    const swingAmount = 0.5 + 0.5 * Math.sin(localT * 0.45);
    ctx.fillStyle = C_BLUE;
    ctx.font = 'bold 13.5px Georgia, "Times New Roman", serif';
    ctx.fillText("Shuffle Timing (Even Steps 2, 4, 6, 8 Shift Later for Groove Swing)", 34, 66);

    const barLeft = 42;
    const barW = 636;
    ctx.strokeStyle = C_AXIS;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(barLeft, 102);
    ctx.lineTo(barLeft + barW, 102);
    ctx.stroke();

    for (let i = 0; i < 8; i++) {
      const straightX = barLeft + (i / 7) * barW;
      const isEvenStep = i % 2 === 1;
      const swungX = isEvenStep ? straightX + swingAmount * 26 : straightX;

      // Reference tick
      ctx.strokeStyle = C_MUTED;
      ctx.beginPath();
      ctx.moveTo(straightX, 92);
      ctx.lineTo(straightX, 112);
      ctx.stroke();

      // Swung node
      ctx.fillStyle = isEvenStep ? C_YELLOW : C_GREEN;
      ctx.beginPath();
      ctx.arc(swungX, 102, 8, 0, Math.PI * 2);
      ctx.fill();

      ctx.fillStyle = C_TEXT;
      ctx.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(`Step ${i + 1}`, swungX, 128);
    }

    // Bottom cards: Presets (Q/W), Tempo ([ ]), and Phase Nudge (; ')
    const cards = [
      {
        title: "Genre Presets [Q / W]",
        sub1: "17 Nu-Disco, House,",
        sub2: "Techno & Booth grooves",
        color: C_PURPLE,
      },
      {
        title: "Tempo Control [T / [ / ]]",
        sub1: "Tap Tempo (T) or nudge",
        sub2: "BPM ±1 (Shift: ±0.1 BPM)",
        color: C_YELLOW,
      },
      {
        title: "DJ Booth Sync [; / ' / L]",
        sub1: "Bridge (CDJ Pro DJ Link) &",
        sub2: "MIDI Clock + ±1 ms Nudge",
        color: C_TEAL,
      },
    ];

    for (let c = 0; c < cards.length; c++) {
      const card = cards[c]!;
      const cx = 36 + c * 222;
      const cy = 166;
      ctx.fillStyle = "rgba(18, 26, 42, 0.92)";
      ctx.strokeStyle = card.color;
      ctx.lineWidth = 1.8;
      ctx.beginPath();
      ctx.roundRect(cx, cy, 204, 88, 8);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = card.color;
      ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "left";
      ctx.fillText(card.title, cx + 12, cy + 26);

      ctx.fillStyle = C_TEXT;
      ctx.font = "12px system-ui, -apple-system, sans-serif";
      ctx.fillText(card.sub1, cx + 12, cy + 50);
      ctx.fillText(card.sub2, cx + 12, cy + 68);
    }

    ctx.fillStyle = C_YELLOW;
    ctx.font = 'italic 14px Georgia, "Times New Roman", serif';
    ctx.textAlign = "center";
    ctx.fillText(
      "Tip: Press Backquote (`) anytime to flip to the RMX-1000 Station guide & FX rack",
      360,
      304,
    );
    ctx.textAlign = "left";
  }

  /**
   * RMX-1000 STATION Chapter 1 (0..30s): 4-Section Signal Flow Diagram
   * (X-Pad Sampler → 3-Band Isolator FX → Scene FX → Release FX Lever).
   */
  private drawRmxChapter1SignalFlow(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("RMX-1000 Station · 4-Stage Live Performance Signal Chain", 24, 32);

    const stages = [
      {
        title: "1. X-PAD FX",
        sub1: "4 Drum Pads (Q W E / 1–4)",
        sub2: "Touch Roll Ribbon (5–0)",
        sub3: "Pitch & Pattern Overdub",
        color: C_BLUE,
      },
      {
        title: "2. ISOLATOR FX",
        sub1: "Low / Mid / High (-48..+6 dB)",
        sub2: "Instant Kill Switches",
        sub3: "Cut/Add · Trans · Gate/Drive",
        color: C_TEAL,
      },
      {
        title: "3. SCENE FX",
        sub1: "5 Build-Up FX (Blue)",
        sub2: "5 Break-Down FX (Orange)",
        sub3: "Main Level + Sub Param",
        color: C_YELLOW,
      },
      {
        title: "4. RELEASE FX",
        sub1: "Hold [R] Spring Lever",
        sub2: "Vinyl Brake / Echo / Spin",
        sub3: "Resets Scene FX on Drop!",
        color: C_RED,
      },
    ];

    const activeStage = Math.floor((localT * 0.25) % 4);
    for (let i = 0; i < stages.length; i++) {
      const st = stages[i]!;
      const x = 24 + i * 172;
      const y = 78;
      const w = 154;
      const h = 152;
      const on = i === activeStage;

      ctx.fillStyle = on ? "rgba(88, 196, 221, 0.2)" : "rgba(18, 26, 42, 0.9)";
      ctx.strokeStyle = on ? C_YELLOW : st.color;
      ctx.lineWidth = on ? 2.6 : 1.6;
      ctx.beginPath();
      ctx.roundRect(x, y, w, h, 9);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = on ? C_YELLOW : st.color;
      ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(st.title, x + 12, y + 28);

      ctx.fillStyle = C_TEXT;
      ctx.font = "11.5px system-ui, -apple-system, sans-serif";
      ctx.fillText(st.sub1, x + 12, y + 62);
      ctx.fillText(st.sub2, x + 12, y + 88);
      ctx.fillStyle = C_MUTED;
      ctx.fillText(st.sub3, x + 12, y + 114);

      if (i < stages.length - 1) {
        const ax = x + w + 3;
        const ay = y + h / 2;
        ctx.strokeStyle = C_YELLOW;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(ax, ay);
        ctx.lineTo(ax + 12, ay);
        ctx.stroke();
      }
    }

    ctx.fillStyle = C_GREEN;
    ctx.font = 'italic 14.5px Georgia, "Times New Roman", serif';
    ctx.textAlign = "center";
    ctx.fillText(
      "Press Backquote (`) to toggle between Step Sequencer and RMX-1000 Station at any time",
      360,
      284,
    );
    ctx.textAlign = "left";
  }

  /**
   * RMX-1000 STATION Chapter 2 (30..60s): X-Pad Drum Triggers (Q W E / 1–4),
   * Touch Roll Ribbon (5–0 -> 1/1..1/32), Pitch & Overdub.
   */
  private drawRmxChapter2XPadRibbon(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("RMX-1000 · X-Pad Drum Triggers [Q W E / 1–4] & Roll Ribbon [5–0]", 24, 32);

    // 4 X-Pad slots
    const pads = [
      { slot: "PAD 1 · BD Kick", keys: "Key [1] or [Q]" },
      { slot: "PAD 2 · SD Snare", keys: "Key [2] or [W]" },
      { slot: "PAD 3 · CP Clap", keys: "Key [3] or [E]" },
      { slot: "PAD 4 · CH Hi-Hat", keys: "Key [4] or [Shift+R]" },
    ];
    const activePad = Math.floor((localT * 0.35) % 4);
    for (let p = 0; p < 4; p++) {
      const px = 34 + p * 166;
      const py = 58;
      const on = p === activePad;
      ctx.fillStyle = on ? "rgba(88, 196, 221, 0.28)" : "rgba(20, 30, 46, 0.9)";
      ctx.strokeStyle = on ? C_YELLOW : C_BLUE;
      ctx.lineWidth = on ? 2.4 : 1.5;
      ctx.beginPath();
      ctx.roundRect(px, py, 152, 64, 8);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = C_TEXT;
      ctx.font = "bold 12.5px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(pads[p]!.slot, px + 76, py + 26);

      ctx.fillStyle = on ? C_YELLOW : C_MUTED;
      ctx.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(pads[p]!.keys, px + 76, py + 47);
    }

    // Touch Roll Ribbon (6 subdivisions on keys 5..0)
    const divs = [
      { label: "1/1", key: "Key 5", pulses: 1 },
      { label: "1/2", key: "Key 6", pulses: 2 },
      { label: "1/4", key: "Key 7", pulses: 4 },
      { label: "1/8", key: "Key 8", pulses: 8 },
      { label: "1/16", key: "Key 9", pulses: 16 },
      { label: "1/32", key: "Key 0", pulses: 24 },
    ];
    const activeDivIdx = Math.floor((localT * 0.28) % 6);

    ctx.fillStyle = C_YELLOW;
    ctx.font = 'bold 13px Georgia, "Times New Roman", serif';
    ctx.textAlign = "left";
    ctx.fillText("TOUCH ROLL RIBBON  —  Hold Keys [5, 6, 7, 8, 9, 0] or Slide Across Ribbon:", 34, 154);

    for (let d = 0; d < divs.length; d++) {
      const dx = 34 + d * 110;
      const dy = 168;
      const isRoll = d === activeDivIdx;
      ctx.fillStyle = isRoll ? "rgba(247, 209, 84, 0.3)" : "rgba(18, 28, 44, 0.92)";
      ctx.strokeStyle = isRoll ? C_YELLOW : C_TEAL;
      ctx.lineWidth = isRoll ? 2.4 : 1.4;
      ctx.beginPath();
      ctx.roundRect(dx, dy, 100, 62, 7);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = C_TEXT;
      ctx.font = "bold 15px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.textAlign = "center";
      ctx.fillText(divs[d]!.label, dx + 50, dy + 25);

      ctx.fillStyle = isRoll ? C_YELLOW : C_TEAL;
      ctx.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(divs[d]!.key, dx + 50, dy + 46);
    }

    // Visual pulse train for active roll subdivision
    const pulses = divs[activeDivIdx]!.pulses;
    const trainLeft = 42;
    const trainW = 636;
    ctx.strokeStyle = C_AXIS;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(trainLeft, 276);
    ctx.lineTo(trainLeft + trainW, 276);
    ctx.stroke();

    for (let k = 0; k < pulses; k++) {
      const px = trainLeft + (k / Math.max(1, pulses - 1)) * trainW;
      ctx.strokeStyle = C_YELLOW;
      ctx.lineWidth = 2.5;
      ctx.beginPath();
      ctx.moveTo(px, 276);
      ctx.lineTo(px, 250);
      ctx.stroke();
    }
    ctx.fillStyle = C_MUTED;
    ctx.font = "12px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(
      "Enable LATCH ROLL to hold rolls hands-free · Enable OVERDUB to write X-Pad hits into the 16-step grid",
      360,
      310,
    );
    ctx.textAlign = "left";
  }

  /**
   * RMX-1000 STATION Chapter 3 (60..90s): 3-Band Isolator FX & Scene FX
   * Build-Up / Break-Down curves.
   */
  private drawRmxChapter3IsolatorAndScene(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("RMX-1000 · 3-Band Isolator Kills & 10 Scene FX Build/Break Curves", 24, 32);

    // Left panel: 3-Band Isolator visualization
    ctx.fillStyle = "rgba(18, 25, 40, 0.9)";
    ctx.strokeStyle = C_TEAL;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.roundRect(28, 56, 310, 248, 9);
    ctx.fill();
    ctx.stroke();

    ctx.fillStyle = C_TEAL;
    ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.fillText("ISOLATOR FX (LOW / MID / HIGH)", 44, 82);

    const bands = [
      { name: "LOW (Sub & Kick)", kill: Math.floor(localT / 6) % 3 === 0, color: C_RED },
      { name: "MID (Snare & Body)", kill: Math.floor(localT / 6) % 3 === 1, color: C_YELLOW },
      { name: "HIGH (Hats & Air)", kill: Math.floor(localT / 6) % 3 === 2, color: C_BLUE },
    ];
    for (let b = 0; b < bands.length; b++) {
      const band = bands[b]!;
      const by = 104 + b * 62;
      ctx.fillStyle = C_TEXT;
      ctx.font = "12px system-ui, sans-serif";
      ctx.fillText(band.name, 44, by + 14);

      ctx.fillStyle = "rgba(11, 13, 20, 0.9)";
      ctx.fillRect(44, by + 22, 190, 16);
      const fillW = band.kill ? 8 : 155;
      ctx.fillStyle = band.kill ? C_RED : band.color;
      ctx.fillRect(44, by + 22, fillW, 16);

      ctx.fillStyle = band.kill ? C_RED : C_MUTED;
      ctx.font = "bold 11px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(band.kill ? "KILL (-48dB)" : "0 dB LIVE", 244, by + 34);
    }

    // Right panel: Scene FX Build-Up vs Break-Down
    ctx.fillStyle = "rgba(18, 25, 40, 0.9)";
    ctx.strokeStyle = C_BLUE;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.roundRect(362, 56, 330, 248, 9);
    ctx.fill();
    ctx.stroke();

    ctx.fillStyle = C_BLUE;
    ctx.font = "bold 13px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.fillText("SCENE FX · BUILD UP vs BREAK DOWN", 378, 82);

    const buildProgress = (localT * 0.12) % 1;
    const gx = 382;
    const gy = 228;
    const gw = 290;
    const gh = 110;

    ctx.strokeStyle = C_AXIS;
    ctx.beginPath();
    ctx.moveTo(gx, gy);
    ctx.lineTo(gx + gw, gy);
    ctx.moveTo(gx, gy);
    ctx.lineTo(gx, gy - gh);
    ctx.stroke();

    // Rising Build-Up curve
    ctx.strokeStyle = C_BLUE;
    ctx.lineWidth = 2.4;
    ctx.beginPath();
    for (let ix = 0; ix <= gw; ix++) {
      const u = ix / gw;
      const y = gy - u * u * gh;
      if (ix === 0) ctx.moveTo(gx + ix, y);
      else ctx.lineTo(gx + ix, y);
    }
    ctx.stroke();

    const dotX = gx + buildProgress * gw;
    const dotY = gy - buildProgress * buildProgress * gh;
    ctx.fillStyle = C_YELLOW;
    ctx.beginPath();
    ctx.arc(dotX, dotY, 6, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = C_TEXT;
    ctx.font = "11.5px system-ui, sans-serif";
    ctx.fillText("Build Up: BPF Echo · Echo · Noise · Spiral Up · Reverb Up", 378, 256);
    ctx.fillStyle = C_MUTED;
    ctx.fillText("Break Down: HPF/LPF/Crush Echo · Spiral/Reverb Down", 378, 278);
  }

  /**
   * RMX-1000 STATION Chapter 4 (90..120s): Spring-Loaded Release FX Lever (Hold R)
   * showing Build-Up tension -> Hold R (Vinyl Brake / Echo / Back Spin) -> Release Drop!
   */
  private drawRmxChapter4ReleaseLever(
    ctx: CanvasRenderingContext2D,
    _W: number,
    _H: number,
    localT: number,
  ): void {
    ctx.fillStyle = C_TEXT;
    ctx.font = 'italic 18px Georgia, "Times New Roman", serif';
    ctx.fillText("RMX-1000 · Spring-Loaded Release FX Lever [Hold R] & Drop", 24, 32);

    const cycle = (localT % 15) / 15; // 15-second repeating performance cycle
    const isBuild = cycle < 0.55;
    const isHoldR = (cycle >= 0.55 && cycle < 0.8) || this.activeCodes.has("KeyR");

    const phaseTitle = isHoldR
      ? "PHASE 2: HOLD [R] — RELEASE FX ENGAGED (Dry Mix Muted · Turntable Spin/Brake)"
      : isBuild
        ? "PHASE 1: RAISE SCENE FX — Building Tension Toward the Drop"
        : "PHASE 3: LET GO OF [R] — Scene FX Resets to 0% & Full Beat Drops Cleanly!";

    ctx.fillStyle = isHoldR ? C_RED : isBuild ? C_BLUE : C_GREEN;
    ctx.font = "bold 13.5px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.fillText(phaseTitle, 32, 68);

    // Timeline graph of Scene FX Level & Release Lever state
    const gx = 42;
    const gy = 216;
    const gw = 636;
    const gh = 110;

    ctx.strokeStyle = C_AXIS;
    ctx.lineWidth = 1.4;
    ctx.beginPath();
    ctx.moveTo(gx, gy);
    ctx.lineTo(gx + gw, gy);
    ctx.stroke();

    ctx.strokeStyle = C_BLUE;
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    for (let ix = 0; ix <= gw; ix++) {
      const u = ix / gw;
      let level = 0;
      if (u < 0.55) {
        level = u / 0.55;
      } else if (u < 0.8) {
        level = 1 - (u - 0.55) / 0.25;
      } else {
        level = 0;
      }
      const py = gy - level * gh;
      if (ix === 0) ctx.moveTo(gx + ix, py);
      else ctx.lineTo(gx + ix, py);
    }
    ctx.stroke();

    // Playhead marker on the curve
    const curX = gx + cycle * gw;
    ctx.strokeStyle = C_YELLOW;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(curX, gy - gh - 12);
    ctx.lineTo(curX, gy + 12);
    ctx.stroke();

    // 3 Release FX mode badges at bottom
    const modes = [
      { name: "VINYL BRAKE", desc: "Turntable motor power-off pitch slowdown" },
      { name: "ECHO OUT", desc: "regenerative dub delay tail into silence" },
      { name: "BACK SPIN", desc: "Rapid platter whip-back sweep" },
    ];
    for (let m = 0; m < modes.length; m++) {
      const mx = 38 + m * 218;
      const my = 244;
      ctx.fillStyle = isHoldR ? "rgba(252, 98, 85, 0.22)" : "rgba(18, 26, 42, 0.9)";
      ctx.strokeStyle = isHoldR ? C_RED : C_YELLOW;
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.roundRect(mx, my, 204, 64, 7);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = isHoldR ? C_YELLOW : C_TEXT;
      ctx.font = "bold 12.5px ui-monospace, SFMono-Regular, Menlo, monospace";
      ctx.fillText(modes[m]!.name, mx + 12, my + 24);

      ctx.fillStyle = C_MUTED;
      ctx.font = "11px system-ui, sans-serif";
      ctx.fillText(modes[m]!.desc, mx + 12, my + 45);
    }
  }
}

export function howToModalMarkup(): string {
  const chapterButtons = SEQ_CHAPTERS.map(
    (c) =>
      `<button type="button" class="howto-ch-btn" data-howto-chapter="${c.id}" aria-pressed="${c.id === 0}">${c.title}</button>`,
  ).join("");

  const keyboardLegend = MACBOOK_ROWS.map(
    (row, rIdx) =>
      `<div class="kb-row kb-row-${rIdx}">${row
        .map(
          (k) =>
            `<button type="button" class="kb-cap" data-key-code="${k.code}" data-key-label="${k.label}" data-seq-sub="${k.seqSub}" data-rmx-sub="${k.rmxSub}" data-key-desc="${k.label} → ${k.seqSub}" title="${k.label}: ${k.seqSub}"><kbd>${k.label}</kbd><span>${k.seqSub}</span></button>`,
        )
        .join("")}</div>`,
  ).join("");

  return `
  <div id="how-to-modal" class="howto-backdrop" role="dialog" aria-modal="true" aria-labelledby="howto-title" data-guide-mode="sequencer" hidden>
    <div class="panel howto-modal">
      <div class="panel-head howto-head">
        <div>
          <span class="howto-badge">Visual Guide &amp; Quick Start · MacBook Air</span>
          <h2 id="howto-title">How to Use player5 &amp; Keyboard Shortcuts</h2>
        </div>
        <div class="howto-mode-tabs" role="group" aria-label="Choose guide section">
          <button id="howto-mode-seq" type="button" class="howto-mode-tab" aria-pressed="true">Step Sequencer Guide</button>
          <button id="howto-mode-rmx" type="button" class="howto-mode-tab" aria-pressed="false">RMX-1000 Station Guide</button>
        </div>
        <button id="howto-close" type="button" class="small" aria-label="Close How To guide">Close (Esc)</button>
      </div>

      <section class="howto-quickstart" id="howto-quickstart" aria-label="Step-by-step quick start">
        <div class="panel-head">
          <h3 id="howto-qs-title">How to Get Started Quickly (Step by Step)</h3>
          <output id="howto-live-key" class="howto-live-key" aria-live="polite">Press any MacBook key to test it live</output>
        </div>

        <ol class="howto-steps" id="howto-qs-seq">
          <li>
            <strong>1. Start the beat &amp; choose a classic TR drum kit:</strong>
            Press <kbd>Space</kbd> (or click <strong>Play</strong>) to start audio. Pick a groove from <strong>Presets</strong> (<em>Nu-Disco</em>, <em>House</em>, or <em>Techno</em>) and select an authentic drum machine kit from <strong>Drum kit</strong> (<strong>TR-8O8 · Sub Boom</strong>, <strong>TR-9O9 · Club Punch</strong>, <strong>TR-7O7 · Studio Matrix</strong>, <strong>TR-6O6 · Acid Silver</strong>, <strong>TR-9O9 · Rave Overdrive</strong>, <strong>TR-72x · Latin Percussion</strong>, or <strong>CR-78 · Vintage Rhythm</strong> — or press <kbd>Y</kbd> <kbd>U</kbd> <kbd>I</kbd> <kbd>O</kbd> <kbd>P</kbd>).
          </li>
          <li>
            <strong>2. Select, play, or mute the 10 drum voices (<kbd>1</kbd>–<kbd>0</kbd>):</strong>
            Press number keys <kbd>1</kbd>–<kbd>0</kbd> (or click <strong>BD</strong>, <strong>SD</strong>, <strong>CP</strong>, <strong>CH</strong> on the left) to select which drum row you are editing. Hold <kbd>Shift</kbd> + <kbd>1</kbd>–<kbd>0</kbd> to play that drum live with your fingers, or <kbd>⌥ Option</kbd> + <kbd>1</kbd>–<kbd>0</kbd> to mute/unmute it.
          </li>
          <li>
            <strong>3. Program the 16 steps (<kbd>A</kbd>–<kbd>K</kbd> &amp; <kbd>Z</kbd>–<kbd>,</kbd>):</strong>
            Your MacBook Home Row <kbd>A</kbd>–<kbd>K</kbd> controls <strong>Steps 1–8</strong>, and the Bottom Row <kbd>Z</kbd>–<kbd>,</kbd> controls <strong>Steps 9–16</strong>. Tap once for a <em>Hit</em>, twice for a louder <em>Accent</em>, and a third time to turn it <em>Off</em> (hold <kbd>Shift</kbd> + step key or press <kbd>E</kbd> for a double-hit <em>Flam</em>).
          </li>
          <li>
            <strong>4. Sculpt the TR voice circuit, swing &amp; DJ booth lock:</strong>
            Use the <strong>Voice</strong> sliders (<em>Tune</em>, <em>Decay</em>, <em>Tone</em>, <em>Snappy</em>) below the grid to shape the selected drum, raise <strong>Shuffle</strong> for house/disco swing, and use the <strong>Clock</strong> panel (<kbd>T</kbd> Tap, <strong>Bridge</strong>, or <strong>MIDI</strong>) plus <kbd>[</kbd> <kbd>]</kbd> and <kbd>;</kbd> <kbd>'</kbd> to lock phase with Pioneer CDJs.
          </li>
        </ol>

        <ol class="howto-steps" id="howto-qs-rmx" hidden>
          <li>
            <strong>1. Open the RMX-1000 Station &amp; trigger X-Pad drums (<kbd>\`</kbd> / <kbd>Q</kbd> <kbd>W</kbd> <kbd>E</kbd> / <kbd>1</kbd>–<kbd>4</kbd>):</strong>
            Click <strong>RMX-1000 Station</strong> in the top bar (or press <kbd>\`</kbd>). Hit <kbd>Q</kbd> <kbd>W</kbd> <kbd>E</kbd> or <kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd> <kbd>4</kbd> to trigger the 4 <strong>X-Pad</strong> drum slots live, and toggle <strong>Bank A / Bank B</strong> to switch between <em>BD/SD/CP/CH</em> and <em>LT/MT/HT/CB</em>.
          </li>
          <li>
            <strong>2. Perform Touch-Ribbon Beat Rolls &amp; Overdub (<kbd>5</kbd>–<kbd>0</kbd>):</strong>
            Hold number keys <kbd>5</kbd> <kbd>6</kbd> <kbd>7</kbd> <kbd>8</kbd> <kbd>9</kbd> <kbd>0</kbd> (or drag across the <strong>Touch Roll Ribbon</strong>) to fire tempo-locked <strong>1/1</strong>, <strong>1/2</strong>, <strong>1/4</strong>, <strong>1/8</strong>, <strong>1/16</strong>, and <strong>1/32</strong> drum rolls. Enable <strong>Latch Roll</strong> to hold rolls hands-free, or turn on <strong>Overdub</strong> to record your X-Pad hits into the 16-step pattern.
          </li>
          <li>
            <strong>3. Carve frequencies with 3-Band Isolator &amp; 10 Scene FX:</strong>
            Punch <strong>KILL LOW</strong>, <strong>KILL MID</strong>, or <strong>KILL HIGH</strong> in <strong>Isolator FX</strong> (and switch between <em>Isolator</em>, <em>Cut/Add</em>, <em>Trans/Roll</em>, and <em>Gate/Drive</em>). Choose a <strong>Build Up</strong> (<em>BPF Echo</em>, <em>Echo</em>, <em>Noise</em>, <em>Spiral Up</em>, <em>Reverb Up</em>) or <strong>Break Down</strong> (<em>HPF/LPF/Crush Echo</em>, <em>Spiral/Reverb Down</em>) <strong>Scene FX</strong> and raise <strong>Scene Level / Time</strong>.
          </li>
          <li>
            <strong>4. Pull the Release FX Lever to drop back on the One (Hold <kbd>R</kbd>):</strong>
            Select <strong>Vinyl Brake</strong>, <strong>Echo</strong>, or <strong>Back Spin</strong>, then hold <kbd>R</kbd> (or hold <strong>Pull Release Lever</strong>) at the climax of your build-up. Letting go of <kbd>R</kbd> automatically clears Scene FX back to 0% and drops the beat cleanly!
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
          <h3 id="howto-kb-title">MacBook Air Key Map · Step Sequencer Mode</h3>
          <span class="hint">Click any key below or press it on your keyboard</span>
        </div>
        <div class="kb-diagram" role="group" aria-label="MacBook Air keyboard shortcuts">
          ${keyboardLegend}
        </div>
      </div>
    </div>
  </div>`;
}
