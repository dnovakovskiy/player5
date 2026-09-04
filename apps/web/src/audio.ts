// Main-thread side of the audio path. Owns the AudioContext and the worklet
// node; forwards patterns and transport, reports the playhead back.

import { specBytes, type PatternSpec } from "./spec";

export type AudioState = "idle" | "starting" | "running" | "failed";

export interface AudioEvents {
  onState(state: AudioState, detail?: string): void;
  onStep(step: number): void;
}

export class AudioEngine {
  private ctx: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  private lastSpec: PatternSpec | null = null;
  private wantPlaying = false;
  state: AudioState = "idle";

  constructor(private readonly events: AudioEvents) {}

  get sampleRate(): number | null {
    return this.ctx?.sampleRate ?? null;
  }

  get playing(): boolean {
    return this.wantPlaying;
  }

  /** Must be called from a user gesture the first time. */
  async ensureStarted(): Promise<void> {
    if (this.ctx && this.node) {
      if (this.ctx.state !== "running") await this.ctx.resume();
      return;
    }
    this.setState("starting");
    try {
      const ctx = new AudioContext({ latencyHint: "interactive" });
      const base = import.meta.env.BASE_URL;
      const [module] = await Promise.all([
        WebAssembly.compileStreaming(fetch(`${base}player5.wasm`)),
        ctx.audioWorklet.addModule(`${base}worklet.js`),
      ]);
      const node = new AudioWorkletNode(ctx, "player5", {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [2],
        processorOptions: { module },
      });
      node.port.onmessage = (e: MessageEvent) => this.onMessage(e.data);
      node.connect(ctx.destination);
      this.ctx = ctx;
      this.node = node;
      if (this.lastSpec) this.send({ type: "pattern", bytes: specBytes(this.lastSpec) });
      await ctx.resume();
      this.setState("running");
    } catch (err) {
      this.setState("failed", err instanceof Error ? err.message : String(err));
      throw err;
    }
  }

  setPattern(spec: PatternSpec): void {
    this.lastSpec = spec;
    if (this.node) this.send({ type: "pattern", bytes: specBytes(spec) });
  }

  async play(): Promise<void> {
    await this.ensureStarted();
    this.wantPlaying = true;
    this.send({ type: "start" });
  }

  stop(): void {
    this.wantPlaying = false;
    this.send({ type: "stop" });
    this.events.onStep(-1);
  }

  private send(msg: unknown): void {
    this.node?.port.postMessage(msg);
  }

  private onMessage(msg: { type: string; step?: number; message?: string }): void {
    if (msg.type === "step" && typeof msg.step === "number") this.events.onStep(msg.step);
    else if (msg.type === "error") this.setState("failed", msg.message);
  }

  private setState(state: AudioState, detail?: string): void {
    this.state = state;
    this.events.onState(state, detail);
  }
}
