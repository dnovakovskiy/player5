// Pioneer RMX-1000 Remix Station emulation audio rack and X-Pad voice/roll
// synthesizer. Sits between the core audio node and AudioContext.destination.
// At default state (Scene FX = 0, Isolator = 0 dB, Release FX disengaged),
// the dry signal passes through at unity gain with flat response.

import type { PatternSpec } from "../spec";
import type { VoiceId } from "../voices";

export type SceneFxId =
  | "bpf_echo"
  | "echo"
  | "noise"
  | "spiral_up"
  | "reverb_up"
  | "hpf_echo"
  | "lpf_echo"
  | "crush_echo"
  | "spiral_down"
  | "reverb_down";

export type IsolatorMode = "isolator" | "cut_add" | "trans_roll" | "gate_drive";
export type ReleaseMode = "vinyl_brake" | "echo" | "back_spin";
export type RollDivision = 1 | 2 | 4 | 8 | 16 | 32;

export interface RmxState {
  sceneFx: SceneFxId | null;
  sceneAmount: number; // 0..1
  sceneSub: number; // 0..1
  isoMode: IsolatorMode;
  isoLow: number; // -48..+6 dB
  isoMid: number; // -48..+6 dB
  isoHigh: number; // -48..+6 dB
  killLow: boolean;
  killMid: boolean;
  killHigh: boolean;
  releaseMode: ReleaseMode;
  releaseHeld: boolean;
  xpadVoice: VoiceId;
  xpadPitch: number; // -1..+1 (semitone/octave shift)
  xpadRollDiv: RollDivision;
  xpadRolling: boolean;
  xpadOverdub: boolean;
  xpadBankB: boolean;
}

export function defaultRmxState(): RmxState {
  return {
    sceneFx: "bpf_echo",
    sceneAmount: 0,
    sceneSub: 0.5,
    isoMode: "isolator",
    isoLow: 0,
    isoMid: 0,
    isoHigh: 0,
    killLow: false,
    killMid: false,
    killHigh: false,
    releaseMode: "vinyl_brake",
    releaseHeld: false,
    xpadVoice: "snare",
    xpadPitch: 0,
    xpadRollDiv: 8,
    xpadRolling: false,
    xpadOverdub: false,
    xpadBankB: false,
  };
}

function makeCrushCurve(amount: number): Float32Array<ArrayBuffer> {
  const n = 1024;
  const curve = new Float32Array(n);
  const steps = Math.max(3, Math.round(32 - amount * 28));
  const drive = 1 + amount * 2.5;
  for (let i = 0; i < n; i++) {
    const x = ((i * 2) / (n - 1) - 1) * drive;
    const q = Math.round(x * steps) / steps;
    curve[i] = Math.max(-0.95, Math.min(0.95, q));
  }
  return curve;
}

function makeDriveCurve(amount: number): Float32Array<ArrayBuffer> {
  const n = 1024;
  const curve = new Float32Array(n);
  const k = 1 + amount * 8;
  const norm = Math.tanh(k);
  for (let i = 0; i < n; i++) {
    const x = (i * 2) / (n - 1) - 1;
    curve[i] = Math.tanh(x * k) / norm;
  }
  return curve;
}

function makeImpulseBuffer(ctx: AudioContext): AudioBuffer {
  const sr = ctx.sampleRate;
  const len = Math.floor(sr * 1.6);
  const buf = ctx.createBuffer(2, len, sr);
  let seed = 0x13579bdf;
  const nextRand = () => {
    seed ^= seed << 13;
    seed ^= seed >>> 17;
    seed ^= seed << 5;
    return (seed >>> 0) / 0xffffffff * 2 - 1;
  };
  for (let ch = 0; ch < 2; ch++) {
    const data = buf.getChannelData(ch);
    for (let i = 0; i < len; i++) {
      const t = i / len;
      const env = (1 - t) ** 2.6;
      data[i] = nextRand() * env * 0.45;
    }
  }
  return buf;
}

function makeNoiseBuffer(ctx: AudioContext): AudioBuffer {
  const sr = ctx.sampleRate;
  const len = sr * 2;
  const buf = ctx.createBuffer(1, len, sr);
  const data = buf.getChannelData(0);
  let seed = 0x2468ace1;
  for (let i = 0; i < len; i++) {
    seed ^= seed << 13;
    seed ^= seed >>> 17;
    seed ^= seed << 5;
    data[i] = ((seed >>> 0) / 0xffffffff) * 2 - 1;
  }
  return buf;
}

export class RmxFxRack {
  readonly input: GainNode;
  private readonly kitLowFilter: BiquadFilterNode;
  private readonly kitTopFilter: BiquadFilterNode;
  private readonly kitShaper: WaveShaperNode;
  private readonly xpadBus: GainNode;
  private readonly isoLowFilter: BiquadFilterNode;
  private readonly isoMidFilter: BiquadFilterNode;
  private readonly isoHighFilter: BiquadFilterNode;
  private readonly isoShaper: WaveShaperNode;
  private readonly isoGateGain: GainNode;
  private readonly sceneToneFilter: BiquadFilterNode;
  private readonly sceneCrusher: WaveShaperNode;
  private readonly dryGain: GainNode;
  private readonly delayNode: DelayNode;
  private readonly delayFeedback: GainNode;
  private readonly delayFilter: BiquadFilterNode;
  private readonly delayWet: GainNode;
  private readonly convolver: ConvolverNode;
  private readonly reverbFilter: BiquadFilterNode;
  private readonly reverbWet: GainNode;
  private readonly noiseFilter: BiquadFilterNode;
  private readonly noiseGain: GainNode;
  private readonly noiseSource: AudioBufferSourceNode;
  private readonly releaseFilter: BiquadFilterNode;
  private readonly releaseGain: GainNode;
  private readonly analyser: AnalyserNode;
  private readonly meterBuf: Float32Array<ArrayBuffer>;
  private readonly noiseBuf: AudioBuffer;
  private lfoTimer: ReturnType<typeof setInterval> | null = null;
  private lfoPhase = 0;
  private bpm = 124;
  private state: RmxState = defaultRmxState();
  private activeOpenHatGain: GainNode | null = null;

  constructor(private readonly ctx: AudioContext) {
    this.input = ctx.createGain();
    this.kitLowFilter = ctx.createBiquadFilter();
    this.kitLowFilter.type = "peaking";
    this.kitLowFilter.frequency.value = 60;
    this.kitLowFilter.Q.value = 0.85;
    this.kitLowFilter.gain.value = 0;

    this.kitTopFilter = ctx.createBiquadFilter();
    this.kitTopFilter.type = "peaking";
    this.kitTopFilter.frequency.value = 4500;
    this.kitTopFilter.Q.value = 0.85;
    this.kitTopFilter.gain.value = 0;

    this.kitShaper = ctx.createWaveShaper();

    this.xpadBus = ctx.createGain();
    this.xpadBus.gain.value = 0.85;

    // 1. 3-Band Isolator FX
    this.isoLowFilter = ctx.createBiquadFilter();
    this.isoLowFilter.type = "lowshelf";
    this.isoLowFilter.frequency.value = 250;
    this.isoLowFilter.gain.value = 0;

    this.isoMidFilter = ctx.createBiquadFilter();
    this.isoMidFilter.type = "peaking";
    this.isoMidFilter.frequency.value = 1000;
    this.isoMidFilter.Q.value = 0.9;
    this.isoMidFilter.gain.value = 0;

    this.isoHighFilter = ctx.createBiquadFilter();
    this.isoHighFilter.type = "highshelf";
    this.isoHighFilter.frequency.value = 2800;
    this.isoHighFilter.gain.value = 0;

    this.isoShaper = ctx.createWaveShaper();
    this.isoGateGain = ctx.createGain();
    this.isoGateGain.gain.value = 1;

    // 2. Scene FX filter + crusher + echo + reverb + noise jet
    this.sceneToneFilter = ctx.createBiquadFilter();
    this.sceneToneFilter.type = "allpass";
    this.sceneToneFilter.frequency.value = 1000;

    this.sceneCrusher = ctx.createWaveShaper();

    this.dryGain = ctx.createGain();
    this.dryGain.gain.value = 1;

    this.delayNode = ctx.createDelay(2.0);
    this.delayNode.delayTime.value = 0.24;
    this.delayFeedback = ctx.createGain();
    this.delayFeedback.gain.value = 0;
    this.delayFilter = ctx.createBiquadFilter();
    this.delayFilter.type = "bandpass";
    this.delayFilter.frequency.value = 1200;
    this.delayFilter.Q.value = 1.2;
    this.delayWet = ctx.createGain();
    this.delayWet.gain.value = 0;

    this.convolver = ctx.createConvolver();
    this.convolver.buffer = makeImpulseBuffer(ctx);
    this.reverbFilter = ctx.createBiquadFilter();
    this.reverbFilter.type = "highpass";
    this.reverbFilter.frequency.value = 350;
    this.reverbWet = ctx.createGain();
    this.reverbWet.gain.value = 0;

    this.noiseBuf = makeNoiseBuffer(ctx);
    this.noiseSource = ctx.createBufferSource();
    this.noiseSource.buffer = this.noiseBuf;
    this.noiseSource.loop = true;
    this.noiseFilter = ctx.createBiquadFilter();
    this.noiseFilter.type = "bandpass";
    this.noiseFilter.frequency.value = 800;
    this.noiseFilter.Q.value = 4.0;
    this.noiseGain = ctx.createGain();
    this.noiseGain.gain.value = 0;

    // 3. Release FX stage
    this.releaseFilter = ctx.createBiquadFilter();
    this.releaseFilter.type = "lowpass";
    this.releaseFilter.frequency.value = 20000;
    this.releaseGain = ctx.createGain();
    this.releaseGain.gain.value = 1;

    // 4. Output analyser for peak meter when RMX synth/FX are active
    this.analyser = ctx.createAnalyser();
    this.analyser.fftSize = 256;
    this.meterBuf = new Float32Array(this.analyser.fftSize);

    // Wire graph
    this.input.connect(this.kitLowFilter);
    this.kitLowFilter.connect(this.kitTopFilter);
    this.kitTopFilter.connect(this.kitShaper);
    this.kitShaper.connect(this.isoLowFilter);
    this.xpadBus.connect(this.isoLowFilter);

    this.isoLowFilter.connect(this.isoMidFilter);
    this.isoMidFilter.connect(this.isoHighFilter);
    this.isoHighFilter.connect(this.isoShaper);
    this.isoShaper.connect(this.isoGateGain);
    this.isoGateGain.connect(this.sceneToneFilter);
    this.sceneToneFilter.connect(this.sceneCrusher);

    // Dry path through release stage
    this.sceneCrusher.connect(this.dryGain);
    this.dryGain.connect(this.releaseFilter);
    this.releaseFilter.connect(this.releaseGain);

    // Echo loop
    this.sceneCrusher.connect(this.delayNode);
    this.delayNode.connect(this.delayFilter);
    this.delayFilter.connect(this.delayFeedback);
    this.delayFeedback.connect(this.delayNode);
    this.delayFilter.connect(this.delayWet);
    this.delayWet.connect(this.analyser);

    // Reverb send
    this.sceneCrusher.connect(this.convolver);
    this.convolver.connect(this.reverbFilter);
    this.reverbFilter.connect(this.reverbWet);
    this.reverbWet.connect(this.analyser);

    // Noise jet
    this.noiseSource.connect(this.noiseFilter);
    this.noiseFilter.connect(this.noiseGain);
    this.noiseGain.connect(this.releaseFilter);
    this.noiseSource.start();

    this.releaseGain.connect(this.analyser);
    this.analyser.connect(ctx.destination);

    this.lfoTimer = setInterval(() => this.tickModulation(), 25);
  }

  setBpm(bpm: number): void {
    if (Number.isFinite(bpm) && bpm >= 20 && bpm <= 400) {
      this.bpm = bpm;
    }
  }

  /**
   * Configures the analog bus voicing for the active TR drum machine kit.
   */
  setKitCharacter(kitId: string): void {
    const now = this.ctx.currentTime;
    const smooth = 0.025;
    let lowFreq = 60;
    let lowGain = 0;
    let topFreq = 4500;
    let topGain = 0;
    let driveCurve: Float32Array<ArrayBuffer> | null = null;

    switch (kitId) {
      case "sub-boom": // TR-8O8
        lowFreq = 50;
        lowGain = 3.2;
        topFreq = 6200;
        topGain = 1.2;
        driveCurve = makeDriveCurve(0.18);
        break;
      case "club-punch": // TR-9O9
        lowFreq = 70;
        lowGain = 3.4;
        topFreq = 7200;
        topGain = 3.0;
        driveCurve = makeDriveCurve(0.32);
        break;
      case "studio-disco": // TR-7O7
        lowFreq = 64;
        lowGain = 2.0;
        topFreq = 3800;
        topGain = 3.5;
        driveCurve = makeCrushCurve(0.16);
        break;
      case "silver-electro": // TR-6O6
        lowFreq = 78;
        lowGain = 2.2;
        topFreq = 8800;
        topGain = 3.8;
        driveCurve = makeDriveCurve(0.24);
        break;
      case "warehouse-raw": // TR-9O9 Rave Overdrive
        lowFreq = 58;
        lowGain = 4.5;
        topFreq = 5400;
        topGain = 3.2;
        driveCurve = makeDriveCurve(0.62);
        break;
      case "latin-perc": // TR-72x Latin Perc
        lowFreq = 190;
        lowGain = 3.2;
        topFreq = 4800;
        topGain = 3.0;
        break;
      case "vintage-cr78": // CR-78 Vintage Rhythm
        lowFreq = 120;
        lowGain = 2.8;
        topFreq = 2600;
        topGain = -2.5;
        driveCurve = makeDriveCurve(0.14);
        break;
    }

    this.kitLowFilter.frequency.setTargetAtTime(lowFreq, now, smooth);
    this.kitLowFilter.gain.setTargetAtTime(lowGain, now, smooth);
    this.kitTopFilter.frequency.setTargetAtTime(topFreq, now, smooth);
    this.kitTopFilter.gain.setTargetAtTime(topGain, now, smooth);
    this.kitShaper.curve = driveCurve;
  }

  /**
   * Fires authentic TR-series analog/PCM character layers synchronized to the
   * Step Sequencer's current step so each TR kit has unmistakable drum timbres.
   */
  triggerKitStep(kitId: string, step: number, spec: PatternSpec): void {
    if (!kitId || step < 0 || step >= 16) return;
    const now = this.ctx.currentTime;
    const stepChar = (v: VoiceId): string => {
      const voice = spec.voices[v];
      if (!voice || voice.mute || voice.level <= 0.01) return "-";
      return voice.steps[step] ?? "-";
    };
    const isActive = (c: string) => c === "x" || c === "X" || c === "f" || c === "F";
    const velOf = (v: VoiceId, c: string) => {
      const base = c === "X" || c === "F" ? 0.95 : 0.7;
      return base * (spec.voices[v]?.level ?? 0.8);
    };

    if (kitId === "club-punch" || kitId === "warehouse-raw") {
      // TR-9O9 signature layers: hard 2-stage triangle pitch-punch kick + 6-bit metallic hat sizzle
      const kc = stepChar("kick");
      if (isActive(kc)) {
        const v = velOf("kick", kc) * (kitId === "warehouse-raw" ? 0.55 : 0.42);
        const osc = this.ctx.createOscillator();
        const g = this.ctx.createGain();
        osc.type = "triangle";
        osc.frequency.setValueAtTime(290, now);
        osc.frequency.exponentialRampToValueAtTime(68, now + 0.016);
        osc.frequency.exponentialRampToValueAtTime(49, now + 0.09);
        g.gain.setValueAtTime(v, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.14);
        osc.connect(g);
        g.connect(this.xpadBus);
        osc.start(now);
        osc.stop(now + 0.15);
      }
      const sc = stepChar("snare");
      if (isActive(sc)) {
        const v = velOf("snare", sc) * 0.35;
        const n = this.ctx.createBufferSource();
        n.buffer = this.noiseBuf;
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 3400;
        bp.Q.value = 1.1;
        const g = this.ctx.createGain();
        g.gain.setValueAtTime(v, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.17);
        n.connect(bp);
        bp.connect(g);
        g.connect(this.xpadBus);
        n.start(now);
        n.stop(now + 0.18);
      }
    } else if (kitId === "studio-disco") {
      // TR-7O7 studio matrix: gated PCM-style snare punch & crisp cowbell/rim transient
      const sc = stepChar("snare");
      if (isActive(sc)) {
        const v = velOf("snare", sc) * 0.42;
        const n = this.ctx.createBufferSource();
        n.buffer = this.noiseBuf;
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 2600;
        bp.Q.value = 0.9;
        const g = this.ctx.createGain();
        g.gain.setValueAtTime(v, now);
        g.gain.setValueAtTime(v * 0.85, now + 0.065);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.11);
        n.connect(bp);
        bp.connect(g);
        g.connect(this.xpadBus);
        n.start(now);
        n.stop(now + 0.12);
      }
    } else if (kitId === "latin-perc") {
      // TR-72x Latin Percussion: High/Low Bongo & Conga slaps, Agogo bells & Cabasa shaker
      for (const [vid, f0] of [
        ["low_tom", 165],
        ["mid_tom", 245],
        ["high_tom", 370],
      ] as const) {
        const tc = stepChar(vid);
        if (isActive(tc)) {
          const v = velOf(vid, tc) * 0.5;
          const osc = this.ctx.createOscillator();
          const g = this.ctx.createGain();
          osc.type = "sine";
          osc.frequency.setValueAtTime(f0 * 1.35, now);
          osc.frequency.exponentialRampToValueAtTime(f0, now + 0.018);
          g.gain.setValueAtTime(v, now);
          g.gain.exponentialRampToValueAtTime(0.001, now + 0.14);
          osc.connect(g);
          g.connect(this.xpadBus);
          osc.start(now);
          osc.stop(now + 0.15);
        }
      }
      const cbc = stepChar("cowbell");
      if (isActive(cbc)) {
        // Two-tone high Agogo bell
        const v = velOf("cowbell", cbc) * 0.36;
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 3100;
        bp.Q.value = 2.2;
        const g = this.ctx.createGain();
        g.gain.setValueAtTime(v, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.18);
        for (const f of [675, 940]) {
          const sq = this.ctx.createOscillator();
          sq.type = "square";
          sq.frequency.value = f;
          sq.connect(bp);
          sq.start(now);
          sq.stop(now + 0.19);
        }
        bp.connect(g);
        g.connect(this.xpadBus);
      }
    } else if (kitId === "vintage-cr78") {
      // CR-78 Vintage CompuRhythm: hollow woodblock clave on rim & metallic tambourine on cowbell
      const rc = stepChar("rim");
      if (isActive(rc)) {
        const v = velOf("rim", rc) * 0.48;
        const osc = this.ctx.createOscillator();
        const g = this.ctx.createGain();
        osc.type = "sine";
        osc.frequency.setValueAtTime(740, now);
        g.gain.setValueAtTime(v, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.045);
        osc.connect(g);
        g.connect(this.xpadBus);
        osc.start(now);
        osc.stop(now + 0.05);
      }
      const cbc = stepChar("cowbell");
      if (isActive(cbc)) {
        // Tambourine / Guiro metallic shimmer
        const v = velOf("cowbell", cbc) * 0.35;
        const n = this.ctx.createBufferSource();
        n.buffer = this.noiseBuf;
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 6800;
        bp.Q.value = 3.2;
        const g = this.ctx.createGain();
        g.gain.setValueAtTime(v, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.12);
        n.connect(bp);
        bp.connect(g);
        g.connect(this.xpadBus);
        n.start(now);
        n.stop(now + 0.13);
      }
    }
  }

  apply(next: RmxState): void {
    const prevHeld = this.state.releaseHeld;
    this.state = { ...next };
    const now = this.ctx.currentTime;
    const smooth = 0.025;

    // 1. Isolator EQ bands (-48 dB .. +6 dB)
    const lowDb = next.killLow ? -48 : next.isoLow;
    const midDb = next.killMid ? -48 : next.isoMid;
    const highDb = next.killHigh ? -48 : next.isoHigh;
    this.isoLowFilter.gain.setTargetAtTime(lowDb, now, smooth);
    this.isoMidFilter.gain.setTargetAtTime(midDb, now, smooth);
    this.isoHighFilter.gain.setTargetAtTime(highDb, now, smooth);

    if (next.isoMode === "gate_drive") {
      const boost = Math.max(0, (lowDb + midDb + highDb) / 18);
      this.isoShaper.curve = makeDriveCurve(0.35 + boost * 0.65);
    } else {
      this.isoShaper.curve = null;
    }

    // 2. Scene FX
    const amt = next.sceneFx ? Math.max(0, Math.min(1, next.sceneAmount)) : 0;
    const sub = Math.max(0, Math.min(1, next.sceneSub));
    const beatSec = 60 / Math.max(40, Math.min(240, this.bpm));

    // Defaults before FX branch
    let toneType: BiquadFilterType = "allpass";
    let toneFreq = 1000;
    let toneQ = 0.7;
    let delayTime = beatSec * 0.5;
    let delayFb = 0;
    let delayWet = 0;
    let delayFilterFreq = 1400;
    let reverbWet = 0;
    let reverbFreq = 350;
    let reverbType: BiquadFilterType = "highpass";
    let noiseLevel = 0;
    let noiseFreq = 800;
    let noiseQ = 4.0;
    let crush = false;

    if (amt > 0.005 && next.sceneFx) {
      switch (next.sceneFx) {
        case "bpf_echo":
          toneType = "bandpass";
          toneFreq = 220 * 14 ** amt;
          toneQ = 0.8 + sub * 3.5;
          delayTime = beatSec * 0.75;
          delayFb = 0.25 + amt * 0.52;
          delayWet = amt * 0.65;
          delayFilterFreq = toneFreq;
          noiseLevel = amt * sub * 0.08;
          noiseFreq = toneFreq;
          break;
        case "echo":
          delayTime = beatSec * (sub > 0.66 ? 0.25 : sub < 0.33 ? 0.75 : 0.5);
          delayFb = 0.2 + sub * 0.62;
          delayWet = amt * 0.75;
          delayFilterFreq = 1800;
          break;
        case "noise":
          noiseLevel = amt * 0.28;
          noiseFreq = 140 * 32 ** amt;
          noiseQ = 2.0 + sub * 8.0;
          delayTime = beatSec * 0.375;
          delayFb = sub * 0.45;
          delayWet = amt * sub * 0.35;
          break;
        case "spiral_up":
          delayTime = Math.max(0.03, beatSec * 0.5 * (1 - amt * 0.65));
          delayFb = 0.45 + amt * 0.42;
          delayWet = amt * 0.72;
          delayFilterFreq = 400 + amt * 3200;
          noiseLevel = amt * sub * 0.1;
          noiseFreq = delayFilterFreq;
          break;
        case "reverb_up":
          reverbWet = amt * 0.85;
          reverbType = "highpass";
          reverbFreq = 180 + amt * 1800;
          noiseLevel = amt * sub * 0.12;
          noiseFreq = 300 + amt * 4200;
          break;
        case "hpf_echo":
          toneType = "highpass";
          toneFreq = 35 * 65 ** amt;
          toneQ = 0.9 + sub * 3.2;
          delayTime = beatSec * 0.5;
          delayFb = 0.25 + sub * 0.5;
          delayWet = amt * 0.55;
          break;
        case "lpf_echo":
          toneType = "lowpass";
          toneFreq = 16000 * 0.015 ** amt;
          toneQ = 0.9 + sub * 3.4;
          delayTime = beatSec * 0.5;
          delayFb = 0.25 + sub * 0.5;
          delayWet = amt * 0.5;
          delayFilterFreq = Math.max(250, toneFreq);
          break;
        case "crush_echo":
          crush = true;
          delayTime = beatSec * 0.5;
          delayFb = 0.2 + sub * 0.5;
          delayWet = amt * 0.5;
          break;
        case "spiral_down":
          delayTime = Math.min(1.8, beatSec * 0.5 * (1 + amt * 1.15));
          delayFb = 0.45 + amt * 0.42;
          delayWet = amt * 0.72;
          delayFilterFreq = Math.max(180, 2400 - amt * 2000);
          break;
        case "reverb_down":
          toneType = "lowpass";
          toneFreq = 16000 * 0.03 ** (amt * 0.7);
          toneQ = 1.0 + sub * 2.0;
          reverbWet = amt * 0.85;
          reverbType = "lowpass";
          reverbFreq = Math.max(220, 3200 - amt * 2600);
          break;
      }
    }

    this.sceneToneFilter.type = toneType;
    this.sceneToneFilter.frequency.setTargetAtTime(toneFreq, now, smooth);
    this.sceneToneFilter.Q.setTargetAtTime(toneQ, now, smooth);
    this.sceneCrusher.curve = crush ? makeCrushCurve(amt * (0.5 + sub * 0.5)) : null;

    this.delayNode.delayTime.setTargetAtTime(delayTime, now, smooth * 2);
    this.delayFeedback.gain.setTargetAtTime(delayFb, now, smooth);
    this.delayFilter.frequency.setTargetAtTime(delayFilterFreq, now, smooth);
    this.delayWet.gain.setTargetAtTime(delayWet, now, smooth);

    this.reverbFilter.type = reverbType;
    this.reverbFilter.frequency.setTargetAtTime(reverbFreq, now, smooth);
    this.reverbWet.gain.setTargetAtTime(reverbWet, now, smooth);

    this.noiseFilter.frequency.setTargetAtTime(noiseFreq, now, smooth);
    this.noiseFilter.Q.setTargetAtTime(noiseQ, now, smooth);
    this.noiseGain.gain.setTargetAtTime(noiseLevel, now, smooth);

    // 3. Release FX lever transitions
    if (next.releaseHeld && !prevHeld) {
      this.triggerReleaseFx(next.releaseMode, beatSec);
    } else if (!next.releaseHeld && prevHeld) {
      this.releaseFilter.frequency.cancelScheduledValues(now);
      this.releaseGain.gain.cancelScheduledValues(now);
      this.dryGain.gain.cancelScheduledValues(now);
      this.releaseFilter.frequency.setTargetAtTime(20000, now, 0.015);
      this.releaseGain.gain.setTargetAtTime(1, now, 0.015);
      this.dryGain.gain.setTargetAtTime(1, now, 0.015);
    }
  }

  private triggerReleaseFx(mode: ReleaseMode, beatSec: number): void {
    const now = this.ctx.currentTime;
    if (mode === "vinyl_brake") {
      // Sweep filter down & brake dry signal while playing a turntable brake tone
      this.releaseFilter.frequency.setValueAtTime(12000, now);
      this.releaseFilter.frequency.exponentialRampToValueAtTime(45, now + beatSec * 0.85);
      this.releaseGain.gain.setValueAtTime(1, now);
      this.releaseGain.gain.linearRampToValueAtTime(0.0001, now + beatSec * 0.9);

      const osc = this.ctx.createOscillator();
      const g = this.ctx.createGain();
      osc.type = "sawtooth";
      osc.frequency.setValueAtTime(190, now);
      osc.frequency.exponentialRampToValueAtTime(24, now + beatSec * 0.75);
      g.gain.setValueAtTime(0.28, now);
      g.gain.exponentialRampToValueAtTime(0.001, now + beatSec * 0.8);
      osc.connect(g);
      g.connect(this.analyser);
      osc.start(now);
      osc.stop(now + beatSec * 0.82);
    } else if (mode === "echo") {
      // Mute dry loop and throw a decaying 1/2-beat echo tail
      this.dryGain.gain.setTargetAtTime(0.0001, now, 0.02);
      this.delayNode.delayTime.setTargetAtTime(beatSec * 0.5, now, 0.02);
      this.delayFeedback.gain.setTargetAtTime(0.78, now, 0.02);
      this.delayWet.gain.setTargetAtTime(0.85, now, 0.02);
      this.triggerPad(this.state.xpadVoice, this.state.xpadPitch, 0.75);
    } else {
      // Back spin: rapid upward whip + filter cut
      this.releaseGain.gain.setTargetAtTime(0.0001, now + 0.18, 0.04);
      const src = this.ctx.createBufferSource();
      src.buffer = this.noiseBuf;
      const bp = this.ctx.createBiquadFilter();
      bp.type = "bandpass";
      bp.Q.value = 3.5;
      bp.frequency.setValueAtTime(260, now);
      bp.frequency.exponentialRampToValueAtTime(5200, now + 0.22);
      bp.frequency.exponentialRampToValueAtTime(180, now + 0.42);
      const g = this.ctx.createGain();
      g.gain.setValueAtTime(0.38, now);
      g.gain.exponentialRampToValueAtTime(0.001, now + 0.44);
      src.connect(bp);
      bp.connect(g);
      g.connect(this.analyser);
      src.start(now);
      src.stop(now + 0.45);
    }
  }

  private tickModulation(): void {
    const mode = this.state.isoMode;
    const now = this.ctx.currentTime;
    if (mode === "cut_add" || mode === "trans_roll") {
      const rateHz = (this.bpm / 60) * (mode === "trans_roll" ? 4 : 2);
      this.lfoPhase = (this.lfoPhase + 0.025 * rateHz) % 1;
      const open = this.lfoPhase < 0.55 ? 1 : mode === "trans_roll" ? 0.12 : 0.35;
      this.isoGateGain.gain.setTargetAtTime(open, now, 0.008);
    } else {
      this.isoGateGain.gain.setTargetAtTime(1, now, 0.015);
    }
  }

  /**
   * Synthesizes an immediate X-Pad drum hit (with pitch shift -1..+1) into
   * the RMX-1000 bus using the authentic TR-inspired analog circuit topologies
   * from core/dsp (bridged-T kick, dual-shell snare, 4-burst flutter clap,
   * 6-oscillator inharmonic square-wave hats with choke, dual-square cowbell,
   * dual-ping rimshot, and damped swept toms).
   */
  triggerPad(voice: VoiceId, pitchShift = 0, velocity = 0.9): void {
    const now = this.ctx.currentTime;
    const ratio = 2 ** (pitchShift * 0.75);
    const vel = Math.max(0.1, Math.min(1, velocity));
    const out = this.ctx.createGain();
    out.gain.value = vel;
    out.connect(this.xpadBus);

    switch (voice) {
      case "kick": {
        // Bridged-T sine body with fast initial punch + exponential tail glide
        const osc = this.ctx.createOscillator();
        const shaper = this.ctx.createWaveShaper();
        shaper.curve = makeDriveCurve(0.28);
        const g = this.ctx.createGain();
        osc.type = "sine";
        osc.frequency.setValueAtTime(215 * ratio, now);
        osc.frequency.exponentialRampToValueAtTime(82 * ratio, now + 0.008);
        osc.frequency.exponentialRampToValueAtTime(48 * ratio, now + 0.085);
        g.gain.setValueAtTime(0.95, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.34);
        osc.connect(shaper);
        shaper.connect(g);
        g.connect(out);
        osc.start(now);
        osc.stop(now + 0.35);

        // Pulse click transient through 2.8 kHz low-pass
        const click = this.ctx.createBufferSource();
        click.buffer = this.noiseBuf;
        const lp = this.ctx.createBiquadFilter();
        lp.type = "lowpass";
        lp.frequency.value = 2800 * ratio;
        const cg = this.ctx.createGain();
        cg.gain.setValueAtTime(0.55 * vel, now);
        cg.gain.exponentialRampToValueAtTime(0.001, now + 0.004);
        click.connect(lp);
        lp.connect(cg);
        cg.connect(out);
        click.start(now);
        click.stop(now + 0.006);
        break;
      }
      case "snare": {
        // Dual bridged-T shell resonators (180 Hz & 330 Hz) with 6 ms pitch blip
        for (const [hz, amp, dur] of [
          [180, 0.48, 0.11],
          [330, 0.36, 0.085],
        ] as const) {
          const osc = this.ctx.createOscillator();
          const og = this.ctx.createGain();
          osc.type = "sine";
          osc.frequency.setValueAtTime(hz * 1.45 * ratio, now);
          osc.frequency.exponentialRampToValueAtTime(hz * ratio, now + 0.009);
          og.gain.setValueAtTime(amp, now);
          og.gain.exponentialRampToValueAtTime(0.001, now + dur);
          osc.connect(og);
          og.connect(out);
          osc.start(now);
          osc.stop(now + dur + 0.01);
        }

        // High-passed + low-passed snare wires
        const n = this.ctx.createBufferSource();
        n.buffer = this.noiseBuf;
        const hp = this.ctx.createBiquadFilter();
        hp.type = "highpass";
        hp.frequency.value = 1650 * ratio;
        const lp = this.ctx.createBiquadFilter();
        lp.type = "lowpass";
        lp.frequency.value = 8200;
        const ng = this.ctx.createGain();
        ng.gain.setValueAtTime(0.62, now);
        ng.gain.exponentialRampToValueAtTime(0.001, now + 0.21);
        n.connect(hp);
        hp.connect(lp);
        lp.connect(ng);
        ng.connect(out);
        n.start(now);
        n.stop(now + 0.22);
        break;
      }
      case "clap": {
        // 4 sawtooth-scheduled noise bursts at 0, 10, 21, 31 ms + blooming room tail
        const n = this.ctx.createBufferSource();
        n.buffer = this.noiseBuf;
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 1270 * ratio;
        bp.Q.value = 1.95;
        const hp = this.ctx.createBiquadFilter();
        hp.type = "highpass";
        hp.frequency.value = 420;
        const ng = this.ctx.createGain();
        const strikes = [0, 0.01, 0.021, 0.031];
        for (let i = 0; i < strikes.length; i++) {
          const t = now + strikes[i]!;
          ng.gain.setValueAtTime(0.75, t);
          if (i < strikes.length - 1) {
            ng.gain.exponentialRampToValueAtTime(0.12, now + strikes[i + 1]! - 0.0005);
          }
        }
        ng.gain.exponentialRampToValueAtTime(0.001, now + 0.25);
        n.connect(bp);
        bp.connect(hp);
        hp.connect(ng);
        ng.connect(out);
        n.start(now);
        n.stop(now + 0.26);
        break;
      }
      case "closed_hat":
      case "open_hat": {
        // Choke any ringing open hat when closed hat triggers
        if (voice === "closed_hat" && this.activeOpenHatGain) {
          try {
            this.activeOpenHatGain.gain.cancelScheduledValues(now);
            this.activeOpenHatGain.gain.setTargetAtTime(0.0001, now, 0.004);
          } catch {
            /* ignore */
          }
          this.activeOpenHatGain = null;
        }
        const dur = voice === "open_hat" ? 0.38 : 0.062;
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 8200 * Math.min(1.3, Math.max(0.75, ratio));
        bp.Q.value = 1.8;
        const hp = this.ctx.createBiquadFilter();
        hp.type = "highpass";
        hp.frequency.value = 6400;
        const hg = this.ctx.createGain();
        hg.gain.setValueAtTime(0.32, now);
        hg.gain.exponentialRampToValueAtTime(0.0008, now + dur);
        if (voice === "open_hat") {
          this.activeOpenHatGain = hg;
        }

        // Six inharmonic square-wave oscillators matching core/dsp/src/hats.rs
        const hatFreqs = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
        for (const f of hatFreqs) {
          const sq = this.ctx.createOscillator();
          sq.type = "square";
          sq.frequency.value = f * ratio;
          sq.connect(bp);
          sq.start(now);
          sq.stop(now + dur + 0.01);
        }
        bp.connect(hp);
        hp.connect(hg);
        hg.connect(out);
        break;
      }
      case "cowbell": {
        // Classic two-oscillator square pair (540 Hz + 800 Hz) + 2.53 kHz bandpass + 2-stage VCA
        const bp = this.ctx.createBiquadFilter();
        bp.type = "bandpass";
        bp.frequency.value = 2530 * ratio;
        bp.Q.value = 1.4;
        const cg = this.ctx.createGain();
        cg.gain.setValueAtTime(0.55, now);
        cg.gain.exponentialRampToValueAtTime(0.16, now + 0.028);
        cg.gain.exponentialRampToValueAtTime(0.001, now + 0.24);
        for (const f of [540, 800]) {
          const sq = this.ctx.createOscillator();
          sq.type = "square";
          sq.frequency.value = f * ratio;
          sq.connect(bp);
          sq.start(now);
          sq.stop(now + 0.25);
        }
        bp.connect(cg);
        cg.connect(out);
        break;
      }
      case "rim": {
        // Dual bridged-T ping resonators (470 Hz & 1660 Hz) + 250 Hz highpass
        const hp = this.ctx.createBiquadFilter();
        hp.type = "highpass";
        hp.frequency.value = 250;
        hp.connect(out);
        for (const [hz, amp, dur] of [
          [470, 0.55, 0.036],
          [1660, 0.45, 0.022],
        ] as const) {
          const osc = this.ctx.createOscillator();
          const rg = this.ctx.createGain();
          osc.type = "sine";
          osc.frequency.value = hz * ratio;
          rg.gain.setValueAtTime(amp, now);
          rg.gain.exponentialRampToValueAtTime(0.001, now + dur);
          osc.connect(rg);
          rg.connect(hp);
          osc.start(now);
          osc.stop(now + dur + 0.005);
        }
        break;
      }
      default: {
        // Tuned analog toms (Low 92 Hz, Mid 141 Hz, High 218 Hz) with 1.45x pitch glide
        const baseFreq: Record<string, number> = {
          low_tom: 92,
          mid_tom: 141,
          high_tom: 218,
        };
        const f0 = (baseFreq[voice] ?? 140) * ratio;
        const osc = this.ctx.createOscillator();
        const g = this.ctx.createGain();
        osc.type = "sine";
        osc.frequency.setValueAtTime(f0 * 1.45, now);
        osc.frequency.exponentialRampToValueAtTime(f0, now + 0.045);
        g.gain.setValueAtTime(0.72, now);
        g.gain.exponentialRampToValueAtTime(0.001, now + 0.28);
        osc.connect(g);
        g.connect(out);
        osc.start(now);
        osc.stop(now + 0.29);
        break;
      }
    }
  }

  /** Peak amplitude on the RMX output bus (0..1+). */
  readPeak(): number {
    this.analyser.getFloatTimeDomainData(this.meterBuf);
    let max = 0;
    for (let i = 0; i < this.meterBuf.length; i++) {
      const a = Math.abs(this.meterBuf[i]!);
      if (a > max) max = a;
    }
    return max;
  }

  dispose(): void {
    if (this.lfoTimer) clearInterval(this.lfoTimer);
    this.lfoTimer = null;
    try {
      this.noiseSource.stop();
    } catch {
      /* already stopped */
    }
    try {
      this.analyser.disconnect();
    } catch {
      /* already disconnected */
    }
  }
}
