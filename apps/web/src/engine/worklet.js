// AudioWorkletProcessor hosting the player5 core (ADR-0004, ADR-0010).
//
// Not loaded from a URL: src/engine/audio-engine.ts concatenates host.js
// and this file into one module source and loads it from a Blob URL, so
// the worklet ships inside the main bundle (and the single-file build).
// `EngineHost` comes from host.js in that same module scope.
//
// The whole engine (scheduler + renderer) runs in here, single-threaded,
// driven by the worklet's own sample clock. The main thread never touches
// audio; it posts pattern bytes, transport and clock messages through the
// port, and they are applied between blocks.

/* global EngineHost, AudioWorkletProcessor, registerProcessor, sampleRate, currentFrame */

class Player5Processor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.host = null;
    try {
      const { module } = options.processorOptions;
      const api = new WebAssembly.Instance(module, {}).exports;
      this.host = new EngineHost(api, sampleRate, (msg) => this.port.postMessage(msg));
    } catch (err) {
      this.port.postMessage({ type: "error", message: "worklet: " + (err && err.message ? err.message : err) });
    }
    this.port.onmessage = (e) => {
      if (this.host) this.host.onMessage(e.data);
    };
  }

  process(_inputs, outputs) {
    const host = this.host;
    if (!host) return false;
    if (!host.alive) return false;
    const output = outputs[0];
    if (!output || output.length === 0) return true;
    const mono = host.process(output[0].length, currentFrame);
    for (let c = 0; c < output.length; c++) output[c].set(mono);
    return true;
  }
}

registerProcessor("player5", Player5Processor);
