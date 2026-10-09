// Main-thread side of the audio path. Owns the AudioContext, picks a
// runtime for the core and forwards patterns, transport and clock messages
// to it; reports playhead, meter and clock status back. See ADR-0010.
//
// Runtimes, best first (forced with ?engine=worklet|script|js):
//   worklet  WebAssembly core inside an AudioWorklet. The worklet module is
//            built from bundled source and loaded from a Blob URL, the
//            compiled WebAssembly.Module travels in processorOptions.
//   script   WebAssembly core on the main thread in a ScriptProcessorNode,
//            when the worklet cannot load (insecure origin, CSP blocking
//            blob: modules).
//   js       the wasm2js build of the same core in a ScriptProcessorNode,
//            when the page's CSP forbids compiling WebAssembly.
// In every runtime the engine runs whole and in lockstep off the audio
// callback's own sample clock; the main thread never triggers sound.

import hostSource from "./host.js?raw";
import workletSource from "./worklet.js?raw";
import { EngineHost, type CoreApi, type HostEvent, type HostMessage } from "./host.js";
import { TimeMap } from "./timemap";
import wasmBase64 from "virtual:player5/wasm";
import wasmUrl from "virtual:player5/wasm-url";

export type EngineMode = "worklet" | "script" | "js";
export type AudioState = "idle" | "starting" | "running" | "failed";

export interface EngineStatus {
  step: number;
  peak: number;
  tempo: number;
  beat: number;
  locked: boolean;
  position: number;
}

export interface AudioEvents {
  onState(state: AudioState, detail?: string): void;
  onStep(step: number): void;
  onStatus(status: EngineStatus): void;
}

/** ScriptProcessor buffer: ~43 ms at 48 kHz, well inside the 100 ms lookahead. */
const SCRIPT_BUFFER = 2048;
const READY_TIMEOUT_MS = 4000;

function base64ToBytes(b64: string): Uint8Array<ArrayBuffer> {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Compiles the core. Throws when WebAssembly is unavailable or blocked by CSP. */
async function compileWasm(): Promise<WebAssembly.Module> {
  if (typeof WebAssembly !== "object") throw new Error("no WebAssembly");
  if (__P5_SINGLE__) {
    if (!wasmBase64) throw new Error("wasm not embedded");
    return WebAssembly.compile(base64ToBytes(wasmBase64));
  } else {
    // Content-hashed in a build (assets/player5-<hash>.wasm): JS and wasm
    // of one build always travel together, also through the service worker.
    const url = new URL(wasmUrl, document.baseURI).href;
    try {
      return await WebAssembly.compileStreaming(fetch(url));
    } catch (err) {
      // Wrong MIME type from a plain static server: compile the bytes.
      if (err instanceof WebAssembly.CompileError) throw err;
      const res = await fetch(url);
      if (!res.ok) throw new Error(`player5.wasm: HTTP ${res.status}`);
      return WebAssembly.compile(await res.arrayBuffer());
    }
  }
}

async function loadJsCore(): Promise<CoreApi> {
  const { createCore } = await import("virtual:player5/core-js");
  return createCore() as CoreApi;
}

export function requestedMode(): EngineMode | null {
  try {
    const m = new URLSearchParams(location.search).get("engine");
    return m === "worklet" || m === "script" || m === "js" ? m : null;
  } catch {
    return null;
  }
}

export class AudioEngine {
  private ctx: AudioContext | null = null;
  private node: AudioNode | null = null;
  private worklet: AudioWorkletNode | null = null;
  private script: ScriptProcessorNode | null = null;
  private host: EngineHost | null = null;
  private startPromise: Promise<void> | null = null;
  private wasm: Promise<WebAssembly.Module> | null = null;
  private timeMap: TimeMap | null = null;
  // Desired state, replayed whenever a runtime comes up.
  private patternBytes: Uint8Array | null = null;
  private clockMode = 0;
  private nudgeMs = 0;
  private latencyMs = 0;
  private wantPlaying = false;
  /** What the current runtime was last told (start/stop). */
  private runtimePlaying = false;

  state: AudioState = "idle";
  mode: EngineMode | null = null;
  lastStatus: EngineStatus | null = null;

  constructor(private readonly events: AudioEvents) {}

  get sampleRate(): number | null {
    return this.ctx?.sampleRate ?? null;
  }

  get playing(): boolean {
    return this.wantPlaying;
  }

  get running(): boolean {
    return this.state === "running";
  }

  /**
   * Starts compiling the core right away, before the first Play: no audio
   * yet, but nothing left to fetch later (an open tab keeps working after
   * a new deploy replaced the service worker's cache) and Play is faster.
   */
  prepare(): void {
    if (requestedMode() === "js") return;
    this.wasmModule().catch(() => {
      /* reported when Play falls back to another runtime */
    });
  }

  /** The compiled core, compiled once. A failed fetch is retried; a refused compile (CSP) is not. */
  private wasmModule(): Promise<WebAssembly.Module> {
    if (!this.wasm) {
      const pending = compileWasm();
      this.wasm = pending;
      pending.catch((err: unknown) => {
        if (this.wasm === pending && !(err instanceof WebAssembly.CompileError)) this.wasm = null;
      });
    }
    return this.wasm;
  }

  /**
   * Creates the AudioContext and the engine. Call from a user gesture the
   * first time (autoplay policy): the context is created synchronously,
   * before any await.
   */
  ensureStarted(): Promise<void> {
    if (this.ctx && this.state === "running") {
      return this.ctx.state === "running" ? Promise.resolve() : this.ctx.resume();
    }
    if (!this.startPromise) {
      let ctx: AudioContext;
      try {
        ctx = new AudioContext({ latencyHint: "interactive" });
      } catch (err) {
        this.setState("failed", err instanceof Error ? err.message : String(err));
        return Promise.reject(err);
      }
      this.ctx = ctx;
      this.startPromise = this.boot(ctx).catch((err: unknown) => {
        // Also drops a runtime that attached before the failure.
        this.teardown(err instanceof Error ? err.message : String(err));
        throw err;
      });
    }
    return this.startPromise;
  }

  private async boot(ctx: AudioContext): Promise<void> {
    this.setState("starting");
    this.timeMap = new TimeMap(ctx);
    // Resume right away while the gesture is fresh; runtimes attach later.
    const resumed = ctx.resume().catch(() => {});
    const forced = requestedMode();
    let module: WebAssembly.Module | null = null;
    let wasmError: unknown = null;
    if (forced !== "js") {
      try {
        module = await this.wasmModule();
      } catch (err) {
        wasmError = err;
        if (forced) throw err;
      }
    }
    if (module && (forced === null || forced === "worklet")) {
      try {
        await this.startWorklet(ctx, module);
        this.mode = "worklet";
      } catch (err) {
        if (forced === "worklet") throw err;
        console.info("player5: AudioWorklet unavailable, rendering on the main thread:", err);
      }
    }
    if (!this.mode && module) {
      const api = (await WebAssembly.instantiate(module, {})).exports as unknown as CoreApi;
      this.startScript(ctx, api);
      this.mode = "script";
    }
    if (!this.mode) {
      const api = await loadJsCore();
      this.startScript(ctx, api);
      this.mode = "js";
      if (wasmError) console.info("player5: WebAssembly unavailable, using the JavaScript core:", wasmError);
    }
    this.replay();
    await resumed;
    if (ctx.state !== "running") await ctx.resume();
    this.setState("running");
  }

  private async startWorklet(ctx: AudioContext, module: WebAssembly.Module): Promise<void> {
    if (!ctx.audioWorklet) throw new Error("AudioWorklet unavailable (insecure context?)");
    const blob = new Blob([hostSource, "\n", workletSource], { type: "text/javascript" });
    const url = URL.createObjectURL(blob);
    try {
      await ctx.audioWorklet.addModule(url);
    } finally {
      URL.revokeObjectURL(url);
    }
    const node = new AudioWorkletNode(ctx, "player5", {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [2],
      processorOptions: { module },
    });
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("worklet did not start")), READY_TIMEOUT_MS);
      node.onprocessorerror = () => {
        clearTimeout(timer);
        reject(new Error("worklet processor error"));
      };
      node.port.onmessage = (e: MessageEvent<HostEvent>) => {
        if (e.data.type === "ready") {
          clearTimeout(timer);
          resolve();
        } else if (e.data.type === "error") {
          clearTimeout(timer);
          reject(new Error(e.data.message));
        }
      };
    }).catch((err: unknown) => {
      node.port.onmessage = null;
      throw err;
    });
    node.port.onmessage = (e: MessageEvent<HostEvent>) => this.onHostEvent(e.data);
    node.onprocessorerror = () => this.teardown("audio worklet crashed; press Play to restart");
    node.connect(ctx.destination);
    this.worklet = node;
    this.node = node;
  }

  private startScript(ctx: AudioContext, api: CoreApi): void {
    const host = new EngineHost(api, ctx.sampleRate, (msg) => this.onHostEvent(msg));
    const node = ctx.createScriptProcessor(SCRIPT_BUFFER, 0, 2);
    node.onaudioprocess = (e: AudioProcessingEvent) => {
      const out = e.outputBuffer;
      let mono: Float32Array;
      try {
        mono = host.process(out.length, Math.round(e.playbackTime * ctx.sampleRate));
      } catch (err) {
        // Like a crashed worklet: stop calling into a broken core.
        node.onaudioprocess = null;
        queueMicrotask(() => this.teardown(`engine error: ${err instanceof Error ? err.message : String(err)}`));
        return;
      }
      for (let c = 0; c < out.numberOfChannels; c++) out.getChannelData(c).set(mono);
    };
    node.connect(ctx.destination);
    this.host = host;
    this.script = node;
    this.node = node;
  }

  /** Drops a dead runtime so the next Play builds a fresh one. */
  private teardown(detail: string): void {
    const ctx = this.ctx;
    if (this.worklet) {
      this.worklet.port.onmessage = null;
      this.worklet.onprocessorerror = null;
    }
    if (this.script) this.script.onaudioprocess = null;
    try {
      this.node?.disconnect();
    } catch {
      /* already disconnected */
    }
    this.worklet = null;
    this.script = null;
    this.host = null;
    this.node = null;
    this.timeMap = null;
    this.ctx = null;
    this.startPromise = null;
    this.mode = null;
    this.lastStatus = null;
    this.wantPlaying = false;
    this.runtimePlaying = false;
    if (ctx) void ctx.close().catch(() => {});
    this.events.onStep(-1);
    this.setState("failed", detail);
  }

  /** Sends the desired state to a fresh runtime. */
  private replay(): void {
    if (this.patternBytes) this.send({ type: "pattern", bytes: this.patternBytes });
    this.send({ type: "latency", ms: this.latencyMs });
    this.send({ type: "nudge", ms: this.nudgeMs });
    this.send({ type: "clock-mode", mode: this.clockMode });
    this.runtimePlaying = false;
    this.syncTransport();
  }

  /**
   * Tells the runtime to start or stop only when that differs from what it
   * was last told. A second "start" restarts the bar (and could trigger the
   * first step twice); a Play that resolves after a Stop must not start.
   */
  private syncTransport(): void {
    if (!this.worklet && !this.host) return;
    if (this.wantPlaying === this.runtimePlaying) return;
    this.runtimePlaying = this.wantPlaying;
    this.send({ type: this.wantPlaying ? "start" : "stop" });
  }

  private send(msg: HostMessage): void {
    if (this.worklet) this.worklet.port.postMessage(msg);
    else if (this.host) this.host.onMessage(msg);
  }

  private onHostEvent(msg: HostEvent): void {
    switch (msg.type) {
      case "offset":
        if (this.timeMap) this.timeMap.offset = msg.offset;
        break;
      case "step":
        this.events.onStep(this.wantPlaying ? msg.step : -1);
        break;
      case "status": {
        const { type: _t, offset, ...status } = msg;
        this.lastStatus = status;
        // Also in every status: a lost "offset" message must not leave
        // the clock sources without a time mapping.
        if (this.timeMap) {
          this.timeMap.offset = offset;
          this.timeMap.sample();
        }
        this.events.onStatus(status);
        break;
      }
      case "error":
        // A rejected pattern leaves the engine running on the previous one.
        if (msg.fatal === false) console.warn("player5:", msg.message);
        else this.teardown(msg.message);
        break;
      case "ready":
        break;
    }
  }

  // ---- control surface -------------------------------------------------

  setPattern(bytes: Uint8Array): void {
    this.patternBytes = bytes;
    this.send({ type: "pattern", bytes });
  }

  async play(): Promise<void> {
    this.wantPlaying = true;
    this.syncTransport();
    try {
      await this.ensureStarted();
    } catch (err) {
      this.wantPlaying = false;
      throw err;
    }
    // A fresh runtime was started by replay(); Stop may have come since.
    this.syncTransport();
  }

  stop(): void {
    this.wantPlaying = false;
    this.syncTransport();
    this.events.onStep(-1);
  }

  /** Clock mode code (core/ffi table): 0 internal, 1..4 follow precision. */
  setClockMode(mode: number): void {
    if (mode === this.clockMode) return;
    this.clockMode = mode;
    this.send({ type: "clock-mode", mode });
  }

  setNudgeMs(ms: number): void {
    this.nudgeMs = ms;
    this.send({ type: "nudge", ms });
  }

  setLatencyMs(ms: number): void {
    this.latencyMs = ms;
    this.send({ type: "latency", ms });
  }

  resync(): void {
    this.send({ type: "resync" });
  }

  /** Engine sample heard at performance time `ms`, or null before audio runs. */
  engineSampleAt(ms: number): number | null {
    return this.state === "running" ? (this.timeMap?.engineSampleAt(ms) ?? null) : null;
  }

  /**
   * The engine's beat being heard at performance time `ms`. Status
   * messages report the beat at the render position, which runs ahead of
   * the speakers by the output latency (and the status is up to 50 ms
   * old); this extrapolates it at the reported tempo. Null before audio.
   */
  beatHeardAt(ms: number): number | null {
    const s = this.lastStatus;
    const sample = this.engineSampleAt(ms);
    const sr = this.sampleRate;
    if (!s || sample === null || !sr) return null;
    return s.beat + ((sample - s.position) / sr) * (s.tempo / 60);
  }

  /** Observation from an external clock at performance time `ms`. */
  observe(ms: number, kind: number, phase: number, bpm: number): boolean {
    const sample = this.engineSampleAt(ms);
    if (sample === null) return false;
    this.send({ type: "observe", sample, kind, phase, bpm });
    return true;
  }

  /** MIDI clock message (0 clock, 1 start, 2 continue, 3 stop) at `ms`. */
  midi(code: number, ms: number): boolean {
    const sample = this.engineSampleAt(ms);
    if (sample === null) return false;
    this.send({ type: "midi", code, sample });
    return true;
  }

  /** Tap at performance time `ms`. */
  tap(ms: number): boolean {
    const sample = this.engineSampleAt(ms);
    if (sample === null) return false;
    this.send({ type: "tap", sample });
    return true;
  }

  private setState(state: AudioState, detail?: string): void {
    this.state = state;
    this.events.onState(state, detail);
  }
}
