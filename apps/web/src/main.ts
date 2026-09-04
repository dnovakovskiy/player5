import "./style.css";
import { AudioEngine, type AudioState } from "./audio";
import {
  cycleStep,
  decodeHash,
  defaultSpec,
  encodeHash,
  normalizeSteps,
  sanitize,
  STEP_COUNT,
  type PatternSpec,
} from "./spec";

// ---- state ----

let spec: PatternSpec = decodeHash(location.hash) ?? defaultSpec();
let playingStep = -1;
const taps: number[] = [];

const audio = new AudioEngine({
  onState: (state, detail) => renderStatus(state, detail),
  onStep: (step) => {
    playingStep = step;
    renderPlayhead();
  },
});

// ---- DOM ----

const app = document.getElementById("app")!;
app.innerHTML = `
  <header class="bar">
    <h1>player<span class="five">5</span></h1>
    <div class="status" id="status" aria-live="polite">audio off</div>
  </header>

  <section class="transport" aria-label="Transport">
    <button id="play" class="play" type="button" aria-pressed="false">Play</button>
    <label class="field bpm">
      <span>BPM</span>
      <div class="bpm-row">
        <button type="button" data-bpm="-1" aria-label="tempo down">−</button>
        <input id="bpm" type="number" inputmode="decimal" min="20" max="400" step="0.1" />
        <button type="button" data-bpm="1" aria-label="tempo up">+</button>
        <button type="button" id="tap">Tap</button>
      </div>
    </label>
    <label class="field"><span>Shuffle <output id="shuffle-out"></output></span>
      <input id="shuffle" type="range" min="0" max="1" step="0.01" /></label>
    <label class="field"><span>Accent <output id="accent-out"></output></span>
      <input id="accent" type="range" min="0" max="1" step="0.01" /></label>
  </section>

  <section class="voice" aria-label="Kick">
    <div class="voice-head"><h2>Kick</h2><span class="hint">tap a step: off → hit → accent</span></div>
    <div class="steps" id="steps" role="group" aria-label="Kick steps"></div>
    <div class="knobs">
      <label class="field"><span>Tune <output id="tune-out"></output></span>
        <input id="tune" type="range" min="0" max="1" step="0.01" /></label>
      <label class="field"><span>Decay <output id="decay-out"></output></span>
        <input id="decay" type="range" min="0" max="1" step="0.01" /></label>
      <label class="field"><span>Level <output id="level-out"></output></span>
        <input id="level" type="range" min="0" max="1" step="0.01" /></label>
    </div>
  </section>

  <section class="master" aria-label="Master">
    <label class="field"><span>Output <output id="gain-out"></output></span>
      <input id="gain" type="range" min="0" max="2" step="0.01" /></label>
    <label class="check"><input id="limiter" type="checkbox" /> <span>Safety limiter</span></label>
    <button id="share" type="button">Copy link</button>
  </section>

  <footer class="foot">
    <span>Space: play/stop · pattern lives in the URL</span>
  </footer>
`;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const playBtn = $<HTMLButtonElement>("play");
const stepsEl = $<HTMLDivElement>("steps");
const bpmInput = $<HTMLInputElement>("bpm");
const shuffle = $<HTMLInputElement>("shuffle");
const accent = $<HTMLInputElement>("accent");
const tune = $<HTMLInputElement>("tune");
const decay = $<HTMLInputElement>("decay");
const level = $<HTMLInputElement>("level");
const gain = $<HTMLInputElement>("gain");
const limiter = $<HTMLInputElement>("limiter");

// Step buttons.
const stepButtons: HTMLButtonElement[] = [];
for (let i = 0; i < STEP_COUNT; i++) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "step";
  b.dataset.step = String(i);
  b.setAttribute("aria-label", `step ${i + 1}`);
  b.addEventListener("click", () => {
    const kick = spec.voices.kick!;
    kick.steps = cycleStep(kick.steps, i);
    commit();
  });
  stepButtons.push(b);
  stepsEl.appendChild(b);
}

// ---- rendering ----

function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}

function renderControls(): void {
  const kick = spec.voices.kick!;
  const steps = normalizeSteps(kick.steps);
  stepButtons.forEach((b, i) => {
    const c = steps[i];
    b.dataset.state = c === "X" ? "accent" : c === "x" ? "on" : "off";
    b.classList.toggle("beat", i % 4 === 0);
  });
  bpmInput.value = String(Math.round(spec.bpm * 10) / 10);
  shuffle.value = String(spec.shuffle);
  accent.value = String(spec.accent);
  tune.value = String(kick.tune);
  decay.value = String(kick.decay);
  level.value = String(kick.level);
  gain.value = String(spec.render?.output_gain ?? 1);
  limiter.checked = Boolean(spec.render?.limiter);
  $("shuffle-out").textContent = pct(spec.shuffle);
  $("accent-out").textContent = pct(spec.accent);
  $("tune-out").textContent = pct(kick.tune);
  $("decay-out").textContent = pct(kick.decay);
  $("level-out").textContent = pct(kick.level);
  $("gain-out").textContent = `${(20 * Math.log10(Math.max(1e-3, spec.render?.output_gain ?? 1))).toFixed(1)} dB`;
}

function renderPlayhead(): void {
  stepButtons.forEach((b, i) => b.classList.toggle("head", i === playingStep));
  app.dataset.playingStep = String(playingStep);
  const playing = audio.playing;
  playBtn.textContent = playing ? "Stop" : "Play";
  playBtn.setAttribute("aria-pressed", String(playing));
}

function renderStatus(state: AudioState, detail?: string): void {
  const el = $("status");
  const sr = audio.sampleRate;
  el.dataset.state = state;
  el.textContent =
    state === "idle" ? "audio off — press Play" :
    state === "starting" ? "starting audio…" :
    state === "running" ? `running · ${sr ? `${sr / 1000} kHz` : ""}` :
    `audio failed: ${detail ?? "unknown"}`;
  app.removeAttribute("aria-busy");
}

/** Apply a UI change everywhere: DOM, audio engine, URL. */
function commit(): void {
  spec = sanitize(spec);
  renderControls();
  audio.setPattern(spec);
  history.replaceState(null, "", encodeHash(spec));
}

// ---- wiring ----

playBtn.addEventListener("click", () => void togglePlay());

async function togglePlay(): Promise<void> {
  if (audio.playing) {
    audio.stop();
  } else {
    try {
      await audio.play();
    } catch {
      /* status already shows the failure */
    }
  }
  renderPlayhead();
}

bpmInput.addEventListener("change", () => {
  spec.bpm = Number(bpmInput.value);
  commit();
});
for (const b of app.querySelectorAll<HTMLButtonElement>("[data-bpm]")) {
  b.addEventListener("click", () => {
    spec.bpm = Math.round(spec.bpm + Number(b.dataset.bpm));
    commit();
  });
}
$("tap").addEventListener("click", () => {
  const now = performance.now();
  if (taps.length && now - taps[taps.length - 1]! > 2000) taps.length = 0;
  taps.push(now);
  if (taps.length > 8) taps.shift();
  if (taps.length >= 2) {
    const span = taps[taps.length - 1]! - taps[0]!;
    spec.bpm = 60000 / (span / (taps.length - 1));
    commit();
  }
});

const bind = (el: HTMLInputElement, apply: (v: number) => void) =>
  el.addEventListener("input", () => {
    apply(Number(el.value));
    commit();
  });
bind(shuffle, (v) => (spec.shuffle = v));
bind(accent, (v) => (spec.accent = v));
bind(tune, (v) => (spec.voices.kick!.tune = v));
bind(decay, (v) => (spec.voices.kick!.decay = v));
bind(level, (v) => (spec.voices.kick!.level = v));
bind(gain, (v) => (spec.render = { ...spec.render, output_gain: v }));
limiter.addEventListener("change", () => {
  spec.render = { ...spec.render, limiter: limiter.checked };
  commit();
});

$("share").addEventListener("click", async () => {
  const url = location.href;
  try {
    await navigator.clipboard.writeText(url);
    $("share").textContent = "Copied";
  } catch {
    prompt("Copy this link", url);
  }
  setTimeout(() => ($("share").textContent = "Copy link"), 1500);
});

window.addEventListener("keydown", (e) => {
  if (e.code === "Space" && !(e.target instanceof HTMLInputElement)) {
    e.preventDefault();
    void togglePlay();
  }
});

window.addEventListener("hashchange", () => {
  const next = decodeHash(location.hash);
  if (next) {
    spec = next;
    renderControls();
    audio.setPattern(spec);
  }
});

// ---- boot ----

commit();
renderPlayhead();
renderStatus("idle");
