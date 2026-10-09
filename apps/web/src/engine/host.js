// The engine host: drives one lockstep P5Engine (core/ffi) block by block.
//
// The same class runs in two places (ADR-0010):
//   * inside the AudioWorklet (preferred), where this file's source is
//     concatenated with worklet.js into a Blob module, and
//   * on the main thread inside a ScriptProcessorNode (fallback), where it
//     is imported as an ordinary module.
// `api` is either a WebAssembly instance's exports or the pure-JS build of
// the same module (wasm2js); both expose the identical p5_* functions.
//
// Plain JavaScript with no imports and no network or console use, because
// its source is shipped verbatim into the worklet. Messages arrive between
// render calls (the worklet's port, or the main thread between audio
// callbacks), so every FFI call made from onMessage happens between blocks.
//
// Engine sample clock: engine samples count the frames this host rendered
// since the engine was created. process() takes the context frame of the
// block it renders (AudioWorkletGlobalScope.currentFrame, or the
// ScriptProcessor's playbackTime * sampleRate) and reports
// `offset = contextFrame - enginePosition` whenever it changes (and in
// every status message); it is constant while the graph runs. The main
// thread maps event times onto engine samples with it
// (src/engine/timemap.ts).

/** Decaying peak meter: about 20 dB per second fall. */
const METER_FALL_DB_PER_S = 20;
/** Status messages per second (tempo, lock, beat, peak). */
const STATUS_HZ = 20;

export class EngineHost {
  /**
   * @param {any} api core exports (wasm instance exports or the JS core)
   * @param {number} sampleRate
   * @param {(msg: any) => void} post sends a message to the main thread
   */
  constructor(api, sampleRate, post) {
    this.api = api;
    this.sampleRate = sampleRate;
    this.post = post;
    this.engine = api.p5_engine_new(sampleRate);
    if (!this.engine) throw new Error("p5_engine_new failed");
    this.frames = 0;
    this.ptr = 0;
    this.view = null;
    this.viewBuffer = null;
    this.offset = null;
    this.lastStep = -2;
    this.peak = 0;
    this.fallPerFrame = Math.pow(10, -METER_FALL_DB_PER_S / 20 / sampleRate);
    this.fallFrames = 0;
    this.fallFactor = 1;
    this.statusEvery = Math.round(sampleRate / STATUS_HZ);
    this.sinceStatus = 0;
    this.alive = true;
    post({ type: "ready", abi: api.p5_abi_version(), sampleRate });
  }

  /** Handles one control message (between render calls). */
  onMessage(msg) {
    if (!this.alive || !msg) return;
    const api = this.api;
    const e = this.engine;
    switch (msg.type) {
      case "pattern": {
        // NUL-terminated UTF-8 JSON, encoded on the main thread.
        const bytes = msg.bytes;
        const ptr = api.p5_alloc(bytes.length);
        new Uint8Array(api.memory.buffer, ptr, bytes.length).set(bytes);
        const rc = api.p5_engine_load_pattern_json(e, ptr);
        api.p5_free(ptr, bytes.length);
        // Not fatal: the engine keeps playing the previous pattern.
        if (rc !== 0) this.post({ type: "error", fatal: false, message: "pattern rejected (" + rc + ")" });
        break;
      }
      case "start":
        api.p5_engine_start(e);
        break;
      case "stop":
        api.p5_engine_stop(e);
        break;
      case "clock-mode":
        api.p5_engine_set_clock_mode(e, msg.mode | 0);
        break;
      case "observe":
        api.p5_engine_observe(e, +msg.sample, msg.kind | 0, +msg.phase, +msg.bpm);
        break;
      case "midi":
        api.p5_engine_midi(e, msg.code | 0, +msg.sample);
        break;
      case "tap":
        api.p5_engine_tap(e, +msg.sample);
        break;
      case "resync":
        api.p5_engine_resync(e);
        break;
      case "nudge":
        api.p5_engine_set_nudge_ms(e, +msg.ms);
        break;
      case "latency":
        api.p5_engine_set_latency_ms(e, +msg.ms);
        break;
      case "dispose":
        api.p5_engine_stop(e);
        this.alive = false;
        break;
    }
    // Anything that can move the timeline (start, tempo, re-sync, tap,
    // mode, nudge) gets a fresh status after the next block, so the main
    // thread's heard-beat estimate never runs on a stale timeline. Clock
    // observations stream continuously and keep the regular rate.
    if (msg.type !== "observe" && msg.type !== "midi") this.sinceStatus = this.statusEvery;
  }

  /**
   * Renders `frames` mono samples and returns a view of them (valid until
   * the next call). `contextFrame` is the audio-context frame this block
   * starts at.
   * @param {number} frames
   * @param {number} contextFrame
   * @returns {Float32Array}
   */
  process(frames, contextFrame) {
    const api = this.api;
    const e = this.engine;
    const position = api.p5_engine_position_f64(e);
    const offset = contextFrame - position;
    if (offset !== this.offset) {
      this.offset = offset;
      this.post({ type: "offset", offset: offset });
    }
    if (frames !== this.frames) {
      if (this.ptr) api.p5_free(this.ptr, this.frames * 4);
      this.frames = frames;
      this.ptr = api.p5_alloc(frames * 4);
      this.view = null;
    }
    api.p5_engine_render(e, this.ptr, frames);
    // The memory's buffer object changes when it grows: re-view only then.
    const buffer = api.memory.buffer;
    if (this.view === null || this.viewBuffer !== buffer) {
      this.view = new Float32Array(buffer, this.ptr, frames);
      this.viewBuffer = buffer;
    }
    const out = this.view;

    // Decaying peak of the rendered (post-master) signal.
    let blockPeak = 0;
    for (let i = 0; i < frames; i++) {
      const a = out[i] < 0 ? -out[i] : out[i];
      if (a > blockPeak) blockPeak = a;
    }
    if (frames !== this.fallFrames) {
      this.fallFrames = frames;
      this.fallFactor = Math.pow(this.fallPerFrame, frames);
    }
    const fallen = this.peak * this.fallFactor;
    this.peak = blockPeak > fallen ? blockPeak : fallen;

    const step = api.p5_engine_playing_step(e);
    if (step !== this.lastStep) {
      this.lastStep = step;
      this.post({ type: "step", step: step });
    }
    this.sinceStatus += frames;
    if (this.sinceStatus >= this.statusEvery) {
      this.sinceStatus = 0;
      this.post({
        type: "status",
        step: step,
        peak: this.peak,
        tempo: api.p5_engine_tempo(e),
        beat: api.p5_engine_beat(e),
        locked: api.p5_engine_clock_locked(e) === 1,
        position: position + frames,
        offset: offset,
      });
    }
    return out;
  }
}
