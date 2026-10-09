// Types for host.js (plain JS so its source can ship verbatim into the
// AudioWorklet).

/** Exports of the core: a WebAssembly instance or the wasm2js build. */
export interface CoreApi {
  memory: { buffer: ArrayBuffer };
  p5_abi_version(): number;
  p5_alloc(bytes: number): number;
  p5_free(ptr: number, bytes: number): void;
  p5_engine_new(sampleRate: number): number;
  p5_engine_free(engine: number): void;
  p5_engine_load_pattern_json(engine: number, json: number): number;
  p5_engine_start(engine: number): void;
  p5_engine_stop(engine: number): void;
  p5_engine_set_stop_after_u32(engine: number, steps: number): void;
  p5_engine_position_f64(engine: number): number;
  p5_engine_playing_step(engine: number): number;
  p5_engine_tempo(engine: number): number;
  p5_engine_beat(engine: number): number;
  p5_engine_clock_locked(engine: number): number;
  p5_engine_set_clock_mode(engine: number, mode: number): number;
  p5_engine_observe(engine: number, sample: number, kind: number, phase: number, bpm: number): number;
  p5_engine_midi(engine: number, message: number, sample: number): number;
  p5_engine_tap(engine: number, sample: number): void;
  p5_engine_resync(engine: number): void;
  p5_engine_set_nudge_ms(engine: number, ms: number): void;
  p5_engine_set_latency_ms(engine: number, ms: number): void;
  p5_engine_render(engine: number, out: number, frames: number): void;
}

/** Main thread → host. */
export type HostMessage =
  | { type: "pattern"; bytes: Uint8Array }
  | { type: "start" }
  | { type: "stop" }
  | { type: "clock-mode"; mode: number }
  | { type: "observe"; sample: number; kind: number; phase: number; bpm: number }
  | { type: "midi"; code: number; sample: number }
  | { type: "tap"; sample: number }
  | { type: "resync" }
  | { type: "nudge"; ms: number }
  | { type: "latency"; ms: number }
  | { type: "dispose" };

/** Host → main thread. */
export type HostEvent =
  | { type: "ready"; abi: number; sampleRate: number }
  | { type: "offset"; offset: number }
  | { type: "step"; step: number }
  | {
      type: "status";
      step: number;
      peak: number;
      tempo: number;
      beat: number;
      locked: boolean;
      position: number;
    }
  | { type: "error"; message: string };

export declare class EngineHost {
  constructor(api: CoreApi, sampleRate: number, post: (msg: HostEvent) => void);
  readonly alive: boolean;
  onMessage(msg: HostMessage): void;
  process(frames: number, contextFrame: number): Float32Array;
}
