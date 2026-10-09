import "./style.css";
import { midiSupported } from "./clock/midi";
import { AudioEngine, type AudioState, type EngineMode, type EngineStatus } from "./engine/audio-engine";
import { DEFAULT_PRESET, PRESETS } from "./presets";
import { setupPwa } from "./pwa";
import {
  BPM_MAX,
  BPM_MIN,
  cycleStep,
  decodeCode,
  decodeHash,
  EMPTY_STEPS,
  encodeCode,
  parsePatternInput,
  sanitize,
  specBytes,
  toggleFlam,
  type PatternSpec,
} from "./spec";
import { loadString, saveString } from "./storage";
import { ClockPanel, type SourceKind } from "./ui/clock-panel";
import { Grid } from "./ui/grid";
import { formatDb, Meter } from "./ui/meter";
import { template } from "./ui/template";
import { CONTROL_LABELS, VOICES, voiceInfo, type ControlId, type VoiceId } from "./voices";

// ---- environment ----

const SINGLE = __P5_SINGLE__;
const bridgeOk = !SINGLE && typeof WebSocket === "function";
const midiOk = !SINGLE && midiSupported();

// ---- state ----

function initialSpec(): PatternSpec {
  let fromHash: PatternSpec | null = null;
  try {
    fromHash = decodeHash(location.hash);
  } catch {
    /* no usable location */
  }
  if (fromHash) return fromHash;
  const saved = loadString("pattern");
  return (saved && decodeCode(saved)) || DEFAULT_PRESET.spec();
}

let spec: PatternSpec = initialSpec();
let selected: VoiceId = "kick";
let flamMode = false;
let playingStep = -1;
const undoStack: string[] = [];
const redoStack: string[] = [];
let lastCheckpoint = { key: "", at: 0 };
/** While the user is typing a tempo, renders leave the BPM field alone. */
let bpmEditing = false;

// ---- DOM ----

const app = document.getElementById("app")!;
app.innerHTML = template({ single: SINGLE, bridge: bridgeOk, midi: midiOk });
if (SINGLE) app.dataset.build = "single";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const playBtn = $<HTMLButtonElement>("play");
const bpmInput = $<HTMLInputElement>("bpm");
const tapBtn = $<HTMLButtonElement>("tap");
const bpmButtons = [...app.querySelectorAll<HTMLButtonElement>("[data-bpm]")];
const shuffle = $<HTMLInputElement>("shuffle");
const accent = $<HTMLInputElement>("accent");
const flam = $<HTMLInputElement>("flam");
const flamModeBtn = $<HTMLButtonElement>("flam-mode");
const presetSelect = $<HTMLSelectElement>("preset");
const undoBtn = $<HTMLButtonElement>("undo");
const gain = $<HTMLInputElement>("gain");
const limiter = $<HTMLInputElement>("limiter");
const voiceControls = $<HTMLDivElement>("voice-controls");
const shareLink = $<HTMLInputElement>("share-link");
const shareCode = $<HTMLInputElement>("share-code");
const importInput = $<HTMLInputElement>("import");
const shareMsg = $<HTMLParagraphElement>("share-msg");

const audio = new AudioEngine({
  onState: (state, detail) => renderStatus(state, detail),
  onStep: (step) => {
    playingStep = audio.playing ? step : -1;
    renderPlayhead();
  },
  onStatus: (status) => onEngineStatus(status),
});

const grid = new Grid($("grid"), {
  onStep(voice, step) {
    checkpoint();
    const v = spec.voices[voice];
    v.steps = flamMode ? toggleFlam(v.steps, step) : cycleStep(v.steps, step);
    commit();
  },
  onSelect(voice) {
    selected = voice;
    renderVoicePanel();
    renderGrid();
  },
  onMute(voice) {
    checkpoint();
    spec.voices[voice].mute = !spec.voices[voice].mute;
    commit();
  },
});

const meter = new Meter($("meter"), $("meter-fill"), $("meter-out"));

const clock = new ClockPanel(
  $("clock"),
  {
    audio,
    tap: (ms) => tap(ms),
    onSourceChange: (source, previous) => onSourceChange(source, previous),
  },
  { bridge: bridgeOk, midi: midiOk },
);

// ---- rendering ----

const pct = (v: number) => `${Math.round(v * 100)}%`;
const gainDb = (g: number) => (g > 0 ? 20 * Math.log10(g) : -Infinity);

function renderGrid(): void {
  grid.render(spec, selected);
}

function renderVoicePanel(): void {
  const info = voiceInfo(selected);
  $("voice-title").textContent = `${info.short} · ${info.name}`;
  const v = spec.voices[selected];
  const existing = voiceControls.dataset.voice === selected;
  if (!existing) {
    voiceControls.dataset.voice = selected;
    voiceControls.replaceChildren(
      ...info.controls.map((c) => {
        const id = `vc-${c}`;
        const label = document.createElement("label");
        label.className = "knob";
        label.htmlFor = id;
        const head = document.createElement("span");
        head.className = "knob-label";
        head.textContent = `${CONTROL_LABELS[c]} `;
        const out = document.createElement("output");
        out.id = `${id}-out`;
        head.append(out);
        const input = document.createElement("input");
        input.id = id;
        input.type = "range";
        input.min = "0";
        input.max = "1";
        input.step = "0.01";
        input.dataset.control = c;
        input.setAttribute("aria-label", `${info.name} ${CONTROL_LABELS[c].toLowerCase()}`);
        input.addEventListener("input", () => {
          checkpoint(`${selected}.${c}`);
          spec.voices[selected][c as ControlId] = Number(input.value);
          commit();
        });
        label.append(head, input);
        return label;
      }),
    );
    const mute = document.createElement("button");
    mute.type = "button";
    mute.className = "toggle voice-mute";
    mute.id = "voice-mute";
    mute.textContent = "Mute";
    mute.addEventListener("click", () => {
      checkpoint();
      spec.voices[selected].mute = !spec.voices[selected].mute;
      commit();
    });
    voiceControls.append(mute);
  }
  for (const input of voiceControls.querySelectorAll<HTMLInputElement>("input[data-control]")) {
    const c = input.dataset.control as ControlId;
    if (input.value !== String(v[c])) input.value = String(v[c]);
    $(`${input.id}-out`).textContent = pct(v[c]);
  }
  const mute = $("voice-mute");
  mute.setAttribute("aria-pressed", String(v.mute));
  mute.setAttribute("aria-label", `Mute ${info.name}`);
}

function renderControls(): void {
  renderGrid();
  renderVoicePanel();
  if (!clock.following && !bpmEditing) bpmInput.value = String(spec.bpm);
  shuffle.value = String(spec.shuffle);
  accent.value = String(spec.accent);
  flam.value = String(spec.flam);
  $("shuffle-out").textContent = pct(spec.shuffle);
  $("accent-out").textContent = pct(spec.accent);
  $("flam-out").textContent = pct(spec.flam);
  const db = gainDb(spec.render.output_gain);
  gain.value = String(Number.isFinite(db) ? Math.max(-24, Math.min(12, db)) : -24);
  $("gain-out").textContent = formatDb(Math.round(db * 10) / 10, "dB");
  limiter.checked = spec.render.limiter;
  flamModeBtn.setAttribute("aria-pressed", String(flamMode));
  app.dataset.flamMode = String(flamMode);
  $("grid-hint").textContent = flamMode
    ? "flam edit: tap toggles a flam"
    : "tap a step: off → hit → accent";
  undoBtn.disabled = undoStack.length === 0;
}

function renderPlayhead(): void {
  grid.setPlayhead(playingStep);
  app.dataset.playingStep = String(playingStep);
  const playing = audio.playing;
  playBtn.textContent = playing ? "Stop" : "Play";
  playBtn.setAttribute("aria-pressed", String(playing));
}

const MODE_LABEL: Record<EngineMode, string> = {
  worklet: "wasm · AudioWorklet",
  script: "wasm · ScriptProcessor",
  js: "JS core · ScriptProcessor",
};

function renderStatus(state: AudioState, detail?: string): void {
  const el = $("status");
  const sr = audio.sampleRate;
  el.dataset.state = state;
  el.textContent =
    state === "idle"
      ? "audio off — press Play"
      : state === "starting"
        ? "starting audio…"
        : state === "running"
          ? `running${sr ? ` · ${sr / 1000} kHz` : ""}`
          : `audio failed: ${detail ?? "unknown"}`;
  if (audio.mode) {
    app.dataset.engine = audio.mode;
    $("engine-mode").textContent = MODE_LABEL[audio.mode];
  }
  app.removeAttribute("aria-busy");
  clock.onAudioState(state);
  if (state !== "running") renderPlayhead();
}

function onEngineStatus(status: EngineStatus): void {
  meter.set(status.peak);
  clock.onStatus(status);
  if (clock.following) bpmInput.value = status.tempo.toFixed(2);
}

/** Apply a change everywhere: DOM, audio engine, share fields, URL, saved copy. */
function commit(): void {
  spec = sanitize(spec);
  renderControls();
  audio.setPattern(specBytes(spec));
  shareCode.value = encodeCode(spec);
  scheduleUrlWrite();
}

// The URL hash and the saved copy follow the pattern at most every 120 ms:
// browsers throttle history.replaceState when a slider drag calls it at
// input-event rate, which would leave a stale link behind.
const URL_WRITE_MS = 120;
let urlTimer: ReturnType<typeof setTimeout> | undefined;
let lastUrlWrite = -Infinity;

function scheduleUrlWrite(): void {
  const wait = URL_WRITE_MS - (performance.now() - lastUrlWrite);
  if (wait <= 0) writeUrl();
  else urlTimer ??= setTimeout(writeUrl, wait);
}

function writeUrl(): void {
  clearTimeout(urlTimer);
  urlTimer = undefined;
  lastUrlWrite = performance.now();
  const code = shareCode.value;
  if (!SINGLE) {
    try {
      if (location.hash !== "#p=" + code) history.replaceState(null, "", "#p=" + code);
    } catch {
      /* sandboxed: the share panel still has the code */
    }
    shareLink.value = location.href;
  }
  saveString("pattern", code);
}

// ---- undo ----

/** Saves the current pattern for undo. Same-key changes within 1.5 s (a slider drag) coalesce. */
function checkpoint(key = ""): void {
  const now = performance.now();
  if (key && key === lastCheckpoint.key && now - lastCheckpoint.at < 1500) {
    lastCheckpoint.at = now;
    return;
  }
  lastCheckpoint = { key, at: now };
  undoStack.push(JSON.stringify(spec));
  if (undoStack.length > 200) undoStack.shift();
  redoStack.length = 0;
}

function undo(): void {
  const prev = undoStack.pop();
  if (prev === undefined) return;
  redoStack.push(JSON.stringify(spec));
  spec = sanitize(JSON.parse(prev));
  lastCheckpoint = { key: "", at: 0 };
  commit();
}

function redo(): void {
  const next = redoStack.pop();
  if (next === undefined) return;
  undoStack.push(JSON.stringify(spec));
  spec = sanitize(JSON.parse(next));
  lastCheckpoint = { key: "", at: 0 };
  commit();
}

// ---- transport ----

async function togglePlay(): Promise<void> {
  if (audio.playing) {
    audio.stop();
  } else {
    try {
      await audio.play();
    } catch {
      /* the status line shows the failure */
    }
  }
  renderPlayhead();
}

playBtn.addEventListener("click", () => void togglePlay());

function setBpm(bpm: number, key: string): void {
  if (!Number.isFinite(bpm)) return;
  checkpoint(key);
  spec.bpm = Math.min(BPM_MAX, Math.max(BPM_MIN, Math.round(bpm * 100) / 100));
  commit();
}

bpmInput.addEventListener("input", () => (bpmEditing = true));
bpmInput.addEventListener("blur", () => {
  bpmEditing = false;
  bpmInput.value = String(spec.bpm);
});
bpmInput.addEventListener("change", () => {
  bpmEditing = false;
  setBpm(Number(bpmInput.value), "bpm");
});
for (const b of bpmButtons) {
  b.addEventListener("click", (e) => {
    const dir = Number(b.dataset.bpm);
    setBpm(e.shiftKey ? spec.bpm + dir * 0.1 : Math.round(spec.bpm) + dir, "bpm");
  });
}

// Tap tempo. The tempo is estimated here (and stored in the pattern); the
// tap's time also goes to the engine, which pulls the beat grid onto it.
const taps: number[] = [];
function tap(ms: number): void {
  if (clock.following) return;
  const last = taps[taps.length - 1];
  if (last !== undefined && (ms - last > 2000 || ms <= last)) taps.length = 0;
  taps.push(ms);
  if (taps.length > 8) taps.shift();
  audio.tap(ms);
  if (taps.length >= 2) {
    const span = taps[taps.length - 1]! - taps[0]!;
    setBpm(60_000 / (span / (taps.length - 1)), "tap");
  }
}
tapBtn.addEventListener("pointerdown", (e) => tap(e.timeStamp));
tapBtn.addEventListener("click", (e) => {
  if (e.detail === 0) tap(performance.now());
});

function onSourceChange(source: SourceKind, previous: SourceKind): void {
  const following = source === "bridge" || source === "midi";
  for (const b of [bpmInput, tapBtn, ...bpmButtons]) b.disabled = following;
  bpmInput.title = following ? `Tempo follows the ${source === "midi" ? "MIDI clock" : "bridge"}` : "";
  const wasFollowing = previous === "bridge" || previous === "midi";
  if (wasFollowing && !following && audio.running && audio.lastStatus) {
    // Keep playing at the tempo we were following instead of jumping back.
    setBpm(audio.lastStatus.tempo, "adopt");
  } else {
    bpmInput.value = String(spec.bpm);
  }
}

const bindUnit = (el: HTMLInputElement, key: string, apply: (v: number) => void) =>
  el.addEventListener("input", () => {
    checkpoint(key);
    apply(Number(el.value));
    commit();
  });
bindUnit(shuffle, "shuffle", (v) => (spec.shuffle = v));
bindUnit(accent, "accent", (v) => (spec.accent = v));
bindUnit(flam, "flam", (v) => (spec.flam = v));
bindUnit(gain, "gain", (db) => (spec.render.output_gain = Math.round(10 ** (db / 20) * 10_000) / 10_000));
limiter.addEventListener("change", () => {
  checkpoint();
  spec.render.limiter = limiter.checked;
  commit();
});

flamModeBtn.addEventListener("click", () => {
  flamMode = !flamMode;
  renderControls();
});

presetSelect.addEventListener("change", () => {
  const preset = PRESETS.find((p) => p.id === presetSelect.value);
  presetSelect.value = "";
  if (!preset) return;
  checkpoint();
  const next = preset.spec();
  next.render = { ...spec.render }; // the booth's master settings stay
  spec = next;
  app.dataset.preset = preset.id;
  commit();
});

$("clear").addEventListener("click", () => {
  checkpoint();
  for (const v of VOICES) spec.voices[v.id].steps = EMPTY_STEPS;
  commit();
});

undoBtn.addEventListener("click", undo);

// ---- share & import ----

let msgTimer: ReturnType<typeof setTimeout> | undefined;
function say(text: string): void {
  shareMsg.textContent = text;
  clearTimeout(msgTimer);
  msgTimer = setTimeout(() => (shareMsg.textContent = ""), 6000);
}

/** Copies via the Clipboard API; otherwise leaves the text selected in its field. */
async function copyField(field: HTMLInputElement, what: string): Promise<void> {
  const text = field.value;
  try {
    if (!navigator.clipboard) throw new Error("no clipboard");
    await navigator.clipboard.writeText(text);
    say(`${what} copied to the clipboard.`);
    return;
  } catch {
    /* fall through: show it selected */
  }
  field.focus();
  field.select();
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }
  say(copied ? `${what} copied.` : `${what} selected: copy it with Ctrl/Cmd+C.`);
}

$("share").addEventListener("click", () => {
  $("share-panel").scrollIntoView({ block: "nearest", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  writeUrl();
  void (SINGLE ? copyField(shareCode, "Pattern code") : copyField(shareLink, "Link"));
});
$("copy-link").addEventListener("click", () => {
  writeUrl();
  void copyField(shareLink, "Link");
});
$("copy-code").addEventListener("click", () => void copyField(shareCode, "Pattern code"));
for (const field of [shareLink, shareCode]) field.addEventListener("focus", () => field.select());

function importText(text: string): boolean {
  const next = parsePatternInput(text);
  if (!next) {
    say("That is not a player5 pattern code or link.");
    return false;
  }
  checkpoint();
  spec = next;
  commit();
  importInput.value = "";
  say("Pattern loaded.");
  return true;
}

importInput.addEventListener("paste", (e) => {
  const text = e.clipboardData?.getData("text") ?? "";
  if (text && importText(text)) e.preventDefault();
});
importInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") importText(importInput.value);
});
$("import-btn").addEventListener("click", () => importText(importInput.value));

if (!SINGLE) {
  window.addEventListener("hashchange", () => {
    const next = decodeHash(location.hash);
    if (next && encodeCode(next) !== encodeCode(spec)) {
      checkpoint();
      spec = next;
      commit();
    }
  });
}

// ---- keyboard ----

function isTextEntry(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement) return true;
  if (t instanceof HTMLInputElement) return !["range", "button", "submit", "reset"].includes(t.type);
  return false;
}

window.addEventListener("keydown", (e) => {
  const mod = e.ctrlKey || e.metaKey;
  if (e.code === "Space" && !mod && !isTextEntry(e.target)) {
    // Space is play/stop everywhere, also on a focused button (Enter
    // activates buttons).
    e.preventDefault();
    if (!e.repeat) void togglePlay();
    return;
  }
  if (mod && !e.altKey && e.key.toLowerCase() === "z" && !isTextEntry(e.target)) {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
    return;
  }
  if (mod && !e.altKey && e.key.toLowerCase() === "y" && !isTextEntry(e.target)) {
    e.preventDefault();
    redo();
    return;
  }
  if (!mod && !e.altKey && (e.key === "t" || e.key === "T") && !isTextEntry(e.target)) {
    if (!e.repeat) tap(e.timeStamp);
  }
});
window.addEventListener("pagehide", () => {
  if (urlTimer !== undefined) writeUrl();
});
window.addEventListener("keyup", (e) => {
  if (e.code === "Space" && e.target instanceof HTMLButtonElement) e.preventDefault();
});

// ---- boot ----

if (!SINGLE) setupPwa($<HTMLButtonElement>("install"));
commit();
renderPlayhead();
renderStatus("idle");
