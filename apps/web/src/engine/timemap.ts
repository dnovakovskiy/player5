// Maps main-thread event times (the performance clock: event.timeStamp,
// MIDIMessageEvent.timeStamp, performance.now()) onto the engine's sample
// clock. This is the crux of following an external clock in a browser
// (ADR-0010):
//
//   1. AudioContext.getOutputTimestamp() pairs {contextTime, performanceTime}:
//      the context time whose audio is leaving the output at that
//      performance time. So for any performance time t (ms):
//          contextTime_at(t) = contextTime + (t - performanceTime) / 1000
//      i.e. the context time of the audio being heard at t.
//   2. frame = contextTime_at(t) * sampleRate is the audio-context frame
//      heard at t.
//   3. The engine counts its own frames from creation. The host reports
//      offset = contextFrame - enginePosition (constant while the graph
//      runs), so: engine sample = frame - offset.
//
// An observation "the source was at beat b at time t" therefore lands on
// the engine sample whose audio is audible at t; the latency offset control
// covers the rest of the path (interface, mixer).
//
// getOutputTimestamp pairs jitter by a callback period or so; the mapping
// keeps a smoothed estimate of (contextTime - performanceTime) and only
// jumps when the two clocks really moved apart (suspend/resume).

const SMOOTHING = 0.05;
const JUMP_S = 0.05;

export class TimeMap {
  /** contextTime - performanceTime / 1000, smoothed. */
  private k: number | null = null;
  private lastSampleMs = -Infinity;
  /** Engine frame offset reported by the host. */
  offset: number | null = null;

  constructor(private readonly ctx: BaseAudioContext & Partial<AudioContext>) {}

  /** Takes one reading of the context/performance clock relation. */
  sample(): void {
    const now = performance.now();
    this.lastSampleMs = now;
    let k: number | null = null;
    const ts = typeof this.ctx.getOutputTimestamp === "function" ? this.ctx.getOutputTimestamp() : null;
    if (ts && ts.contextTime && ts.performanceTime) {
      k = ts.contextTime - ts.performanceTime / 1000;
    } else if (this.ctx.currentTime > 0) {
      // No usable timestamp yet: currentTime is the block being rendered;
      // subtract the reported output latency to approximate what is heard.
      const latency = (this.ctx.outputLatency || 0) + (this.ctx.baseLatency || 0);
      k = this.ctx.currentTime - latency - now / 1000;
    }
    if (k === null || !Number.isFinite(k)) return;
    if (this.k === null || Math.abs(k - this.k) > JUMP_S) this.k = k;
    else this.k += SMOOTHING * (k - this.k);
  }

  /** Context time (s) heard at performance time `ms`, or null. */
  contextTimeAt(ms: number): number | null {
    if (performance.now() - this.lastSampleMs > 20) this.sample();
    return this.k === null ? null : this.k + ms / 1000;
  }

  /** Engine sample position heard at performance time `ms`, or null. */
  engineSampleAt(ms: number): number | null {
    const t = this.contextTimeAt(ms);
    if (t === null || this.offset === null) return null;
    return t * this.ctx.sampleRate - this.offset;
  }
}
