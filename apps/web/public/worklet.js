// AudioWorkletProcessor hosting the player5 core.
//
// The whole engine (scheduler + renderer) runs in here, single-threaded,
// driven by the worklet's own sample clock: the same lockstep mode the
// offline renderer and the Node verification use. The main thread never
// touches audio; it only posts pattern bytes and transport messages
// through the port. Kept as plain JS with no imports so it needs no
// bundling and loads straight from the static site.

const BLOCK_FRAMES = 128;

class Player5Processor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const { module } = options.processorOptions;
    this.instance = new WebAssembly.Instance(module, {});
    this.api = this.instance.exports;
    this.memory = this.api.memory;
    this.engine = this.api.p5_engine_new(sampleRate);
    this.outFrames = BLOCK_FRAMES;
    this.outPtr = this.api.p5_alloc(this.outFrames * 4);
    this.lastStep = -2;
    this.alive = true;

    this.port.onmessage = (e) => this.onMessage(e.data);
    this.port.postMessage({ type: "ready", abi: this.api.p5_abi_version(), sampleRate });
  }

  onMessage(msg) {
    switch (msg.type) {
      case "pattern": {
        // msg.bytes is NUL-terminated UTF-8 JSON, encoded on the main thread.
        const bytes = msg.bytes;
        const ptr = this.api.p5_alloc(bytes.length);
        new Uint8Array(this.memory.buffer, ptr, bytes.length).set(bytes);
        const rc = this.api.p5_engine_load_pattern_json(this.engine, ptr);
        this.api.p5_free(ptr, bytes.length);
        if (rc !== 0) this.port.postMessage({ type: "error", message: `pattern rejected (${rc})` });
        break;
      }
      case "start":
        this.api.p5_engine_start(this.engine);
        break;
      case "stop":
        this.api.p5_engine_stop(this.engine);
        break;
      case "dispose":
        this.api.p5_engine_stop(this.engine);
        this.alive = false;
        break;
    }
  }

  process(_inputs, outputs) {
    if (!this.alive) return false;
    const output = outputs[0];
    if (!output || output.length === 0) return true;
    const frames = output[0].length;
    if (frames !== this.outFrames) {
      this.api.p5_free(this.outPtr, this.outFrames * 4);
      this.outFrames = frames;
      this.outPtr = this.api.p5_alloc(frames * 4);
    }
    this.api.p5_engine_render(this.engine, this.outPtr, frames);
    // The buffer object changes when wasm memory grows, so re-view per block.
    const mono = new Float32Array(this.memory.buffer, this.outPtr, frames);
    for (const channel of output) channel.set(mono);

    const step = this.api.p5_engine_playing_step(this.engine);
    if (step !== this.lastStep) {
      this.lastStep = step;
      this.port.postMessage({ type: "step", step });
    }
    return true;
  }
}

registerProcessor("player5", Player5Processor);
