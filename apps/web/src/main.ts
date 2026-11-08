import "./style.css";
import { midiSupported } from "./clock/midi";
import { AudioEngine, type AudioState, type EngineMode, type EngineStatus } from "./engine/audio-engine";
import {
  applyAutoFill,
  clearAllVoiceFx,
  cloneVoiceFxMap,
  defaultVoiceFxMap,
  GROOVE_TEMPLATES,
  humanizePattern,
  isGroupEchoActive,
  mutatePattern,
  toggleGroupFx,
  type VoiceFxKind,
  type VoiceFxMap,
} from "./groove";
import { applyKitToSpec, DEFAULT_PRESET, KITS, PRESETS } from "./presets";
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
  STEP_COUNT,
  toggleFlam,
  type PatternSpec,
} from "./spec";
import { loadString, saveString } from "./storage";
import { ClockPanel, type SourceKind } from "./ui/clock-panel";
import { Grid } from "./ui/grid";
import { HowToExplainer } from "./ui/how-to";
import { formatDb, Meter } from "./ui/meter";
import { RmxPanel } from "./ui/rmx-panel";
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

interface HistoryEntry {
  spec: PatternSpec;
  presetId: string;
  kitId: string;
  grooveId?: string;
  humanize?: number;
  voiceFx?: VoiceFxMap;
  voiceFxLevel?: number;
}

function findMatchingPresetId(s: PatternSpec): string {
  const code = encodeCode(s);
  return PRESETS.find((p) => encodeCode(p.spec()) === code)?.id ?? "";
}

let spec: PatternSpec = initialSpec();
let activePresetId = findMatchingPresetId(spec);
let activeKitId = PRESETS.find((p) => p.id === activePresetId)?.kitId ?? "";
let activeGrooveId = "";
let humanizeAmount = 0;
let voiceFx: VoiceFxMap = defaultVoiceFxMap();
let voiceFxLevel = 0.72;
let selected: VoiceId = "kick";
let flamMode = false;
/** Step being heard (the playhead). */
let playingStep = -1;
/** Step at the engine's render position, as posted by the audio runtime. */
let renderedStep = -1;
let playheadFrame = 0;
const undoStack: string[] = [];
const redoStack: string[] = [];
let lastCheckpoint = { key: "", at: 0 };
/** While the user is typing a tempo, renders leave the BPM field alone. */
let bpmEditing = false;

// ---- DOM ----

const app = document.getElementById("app")!;
const secure = typeof isSecureContext === "boolean" ? isSecureContext : true;
app.innerHTML = template({ single: SINGLE, bridge: bridgeOk, midi: midiOk, secure });
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
const kitSelect = $<HTMLSelectElement>("kit");
const grooveSelect = $<HTMLSelectElement>("groove-template");
const humanizeInput = $<HTMLInputElement>("humanize");
const vfxLevelInput = $<HTMLInputElement>("vfx-level");
const vfxReadout = $<HTMLOutputElement>("vfx-readout");
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
    renderedStep = audio.playing ? step : -1;
    if (renderedStep < 0) playingStep = -1;
    renderPlayhead();
    if (renderedStep >= 0 && !playheadFrame) playheadFrame = requestAnimationFrame(followPlayhead);
  },
  onStatus: (status) => onEngineStatus(status),
});

function toggleVoiceFx(voice: VoiceId, fx: VoiceFxKind): void {
  checkpoint(`vfx-${voice}-${fx}`);
  voiceFx[voice][fx] = !voiceFx[voice][fx];
  commit();
}

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
    renderGrooveAndFxControls();
  },
  onMute(voice) {
    checkpoint();
    spec.voices[voice].mute = !spec.voices[voice].mute;
    commit();
  },
  onToggleFx(voice, fx) {
    toggleVoiceFx(voice, fx);
  },
});

const meter = new Meter($("meter"), $("meter-fill"), $("meter-out"));

const clock = new ClockPanel(
  $("clock"),
  {
    audio,
    tap: (ms) => tap(ms),
    onSourceChange: (source, previous) => onSourceChange(source, previous),
    getPatternBpm: () => spec.bpm,
  },
  { bridge: bridgeOk, midi: midiOk },
);

const rmx = new RmxPanel($("rmx-panel"), {
  audio,
  getBpm: () => (clock.following && audio.lastStatus ? audio.lastStatus.tempo : spec.bpm),
  getStep: () => playingStep,
  onOverdubHit(voice, stepIdx, isAccent, tuneOffset) {
    checkpoint(`rmx-overdub-${voice}`);
    const v = spec.voices[voice];
    const chars = [...v.steps];
    const index = Math.max(0, Math.min(STEP_COUNT - 1, stepIdx));
    chars[index] = isAccent ? "X" : "x";
    v.steps = chars.join("");
    if (Math.abs(tuneOffset) > 0.01) {
      v.tune = Math.max(0, Math.min(1, Math.round((0.5 + tuneOffset * 0.45) * 100) / 100));
    }
    commit();
  },
  onToggleVoiceMute(voice) {
    checkpoint();
    spec.voices[voice].mute = !spec.voices[voice].mute;
    commit();
  },
  onClearVoice(voice) {
    checkpoint();
    spec.voices[voice].steps = EMPTY_STEPS;
    commit();
  },
  isVoiceMuted: (voice) => spec.voices[voice].mute,
});

const howTo = new HowToExplainer(app);

// ---- surface mode (Step Sequencer vs RMX-1000 Station) ----

type SurfaceMode = "sequencer" | "rmx";
let surfaceMode: SurfaceMode = "sequencer";
app.dataset.surface = surfaceMode;

function setSurfaceMode(mode: SurfaceMode): void {
  surfaceMode = mode;
  app.dataset.surface = mode;
  const isRmx = mode === "rmx";
  $("mode-seq").setAttribute("aria-pressed", String(!isRmx));
  $("mode-rmx").setAttribute("aria-pressed", String(isRmx));
  $("rmx-panel").hidden = !isRmx;
  $("grid-panel").hidden = isRmx;
  $("voice-panel").hidden = isRmx;
  howTo.setGuideMode(mode);
}

$("mode-seq").addEventListener("click", () => setSurfaceMode("sequencer"));
$("mode-rmx").addEventListener("click", () => setSurfaceMode("rmx"));

// ---- rendering ----

const pct = (v: number) => `${Math.round(v * 100)}%`;
const gainDb = (g: number) => (g > 0 ? 20 * Math.log10(g) : -Infinity);

function renderGrid(): void {
  grid.render(spec, selected, voiceFx);
}

function renderGrooveAndFxControls(): void {
  grooveSelect.value = activeGrooveId;
  humanizeInput.value = String(humanizeAmount);
  $("humanize-out").textContent = pct(humanizeAmount);
  vfxLevelInput.value = String(voiceFxLevel);
  $("vfx-level-out").textContent = pct(voiceFxLevel);

  const selFx = voiceFx[selected];
  const selInfo = voiceInfo(selected);
  $("vfx-echo").setAttribute("aria-pressed", String(selFx.echo));
  $("vfx-echo").textContent = `${selInfo.short} Echo [⌥E]`;
  $("vfx-reverb").setAttribute("aria-pressed", String(selFx.reverb));
  $("vfx-reverb").textContent = `${selInfo.short} Reverb [⌥R]`;
  $("vfx-filter").setAttribute("aria-pressed", String(selFx.filter));
  $("vfx-filter").textContent = `${selInfo.short} Filter [⌥D]`;

  $("grp-fx-snare").setAttribute("aria-pressed", String(isGroupEchoActive(voiceFx, "snare_clap")));
  $("grp-fx-hats").setAttribute("aria-pressed", String(isGroupEchoActive(voiceFx, "hats")));
  $("grp-fx-toms").setAttribute("aria-pressed", String(isGroupEchoActive(voiceFx, "toms")));

  // Build concise readout of active per-voice FX
  const activeSummaries: string[] = [];
  for (const v of VOICES) {
    const f = voiceFx[v.id];
    const tags: string[] = [];
    if (f.echo) tags.push("EC");
    if (f.reverb) tags.push("RV");
    if (f.filter) tags.push("FL");
    if (tags.length > 0) activeSummaries.push(`${v.short}:${tags.join("+")}`);
  }
  vfxReadout.textContent =
    activeSummaries.length > 0
      ? `FX ON (${pct(voiceFxLevel)}) · ${activeSummaries.join(" ")}`
      : "DRY · NO VOICE FX";
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
  renderGrooveAndFxControls();
  rmx.render();
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
  presetSelect.value = activePresetId;
  kitSelect.value = activeKitId;
  if (activePresetId) app.dataset.preset = activePresetId;
  else delete app.dataset.preset;
  if (activeKitId) app.dataset.kit = activeKitId;
  else delete app.dataset.kit;
  $("grid-hint").textContent = flamMode
    ? "flam edit: tap toggles a flam"
    : "tap a step: off → hit → accent · EC/RV/FL: per-voice FX";
  undoBtn.disabled = undoStack.length === 0;
}

/** Steps per beat in the core (sequencer::BEATS_PER_STEP = 0.25). */
const STEPS_PER_BEAT = 4;
/**
 * The playhead shows the step being *heard*. The runtime posts the step at
 * its render position, which runs ahead of the speakers by the output
 * latency (tens of ms in a worklet, ~100 ms in the ScriptProcessor
 * fallbacks); the heard beat is the same quantity the core's playing_step
 * uses, taken at the moment the audio leaves the output.
 */
function followPlayhead(): void {
  playheadFrame = 0;
  if (!audio.playing || renderedStep < 0) {
    playingStep = -1;
    renderPlayhead();
    return;
  }
  const beat = audio.beatHeardAt(performance.now());
  const step =
    beat === null ? renderedStep : beat < 0 ? -1 : Math.floor(beat * STEPS_PER_BEAT) % STEP_COUNT;
  if (step !== playingStep) {
    playingStep = step;
    renderPlayhead();
  }
  playheadFrame = requestAnimationFrame(followPlayhead);
}

function renderPlayhead(): void {
  grid.setPlayhead(playingStep);
  if (app.dataset.playingStep !== String(playingStep)) app.dataset.playingStep = String(playingStep);
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
  audio.setKit(activeKitId, spec);
  audio.setVoiceFxAndHumanize(voiceFx, humanizeAmount, voiceFxLevel);
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

function snapshotState(): string {
  const entry: HistoryEntry = {
    spec,
    presetId: activePresetId,
    kitId: activeKitId,
    grooveId: activeGrooveId,
    humanize: humanizeAmount,
    voiceFx: cloneVoiceFxMap(voiceFx),
    voiceFxLevel,
  };
  return JSON.stringify(entry);
}

function restoreSnapshot(raw: string): void {
  const parsed = JSON.parse(raw) as HistoryEntry | PatternSpec;
  if (parsed && typeof parsed === "object" && "spec" in parsed) {
    spec = sanitize(parsed.spec);
    activePresetId = parsed.presetId ?? "";
    activeKitId = parsed.kitId ?? "";
    activeGrooveId = parsed.grooveId ?? "";
    humanizeAmount = typeof parsed.humanize === "number" ? parsed.humanize : 0;
    voiceFx = parsed.voiceFx ? cloneVoiceFxMap(parsed.voiceFx) : defaultVoiceFxMap();
    voiceFxLevel = typeof parsed.voiceFxLevel === "number" ? parsed.voiceFxLevel : 0.72;
  } else {
    spec = sanitize(parsed);
  }
}

/** Saves the current pattern for undo. Same-key changes within 1.5 s (a slider drag) coalesce. */
function checkpoint(key = ""): void {
  const now = performance.now();
  if (key && key === lastCheckpoint.key && now - lastCheckpoint.at < 1500) {
    lastCheckpoint.at = now;
    return;
  }
  lastCheckpoint = { key, at: now };
  undoStack.push(snapshotState());
  if (undoStack.length > 200) undoStack.shift();
  redoStack.length = 0;
}

function undo(): void {
  const prev = undoStack.pop();
  if (prev === undefined) return;
  redoStack.push(snapshotState());
  restoreSnapshot(prev);
  lastCheckpoint = { key: "", at: 0 };
  commit();
}

function redo(): void {
  const next = redoStack.pop();
  if (next === undefined) return;
  undoStack.push(snapshotState());
  restoreSnapshot(next);
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
  const following = source === "link" || source === "bridge" || source === "midi";
  for (const b of [bpmInput, tapBtn, ...bpmButtons]) b.disabled = following;
  bpmInput.title = following
    ? `Tempo follows ${source === "midi" ? "MIDI clock" : source === "link" ? "Ableton Link" : "the bridge"}`
    : "";
  const wasFollowing = previous === "link" || previous === "bridge" || previous === "midi";
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

// ---- Groove, Humanize, Variation & Per-Sample FX bindings ----

grooveSelect.addEventListener("change", () => {
  const groove = GROOVE_TEMPLATES.find((g) => g.id === grooveSelect.value);
  if (!groove) {
    grooveSelect.value = activeGrooveId;
    return;
  }
  checkpoint("groove-template");
  activeGrooveId = groove.id;
  spec.shuffle = groove.shuffle;
  spec.accent = groove.accent;
  spec.flam = groove.flam;
  humanizeAmount = groove.humanize;
  commit();
});

bindUnit(humanizeInput, "humanize", (v) => {
  humanizeAmount = v;
});

bindUnit(vfxLevelInput, "vfx-level", (v) => {
  voiceFxLevel = v;
});

function triggerHumanizeAction(): void {
  checkpoint("var-humanize");
  if (humanizeAmount < 0.25) humanizeAmount = 0.35;
  spec = humanizePattern(spec, "all", Math.max(0.45, humanizeAmount));
  commit();
}

function triggerMutateAction(): void {
  checkpoint("var-mutate");
  spec = mutatePattern(spec, "all", 0.55);
  commit();
}

function triggerAutoFillAction(): void {
  checkpoint("var-fill");
  spec = applyAutoFill(spec);
  commit();
}

$("var-humanize").addEventListener("click", triggerHumanizeAction);
$("var-mutate").addEventListener("click", triggerMutateAction);
$("var-fill").addEventListener("click", triggerAutoFillAction);

$("vfx-echo").addEventListener("click", () => toggleVoiceFx(selected, "echo"));
$("vfx-reverb").addEventListener("click", () => toggleVoiceFx(selected, "reverb"));
$("vfx-filter").addEventListener("click", () => toggleVoiceFx(selected, "filter"));

function triggerGroupEcho(groupId: "snare_clap" | "hats" | "toms"): boolean {
  checkpoint(`grp-fx-${groupId}`);
  const on = toggleGroupFx(voiceFx, groupId, "echo");
  commit();
  return on;
}

$("grp-fx-snare").addEventListener("click", () => triggerGroupEcho("snare_clap"));
$("grp-fx-hats").addEventListener("click", () => triggerGroupEcho("hats"));
$("grp-fx-toms").addEventListener("click", () => triggerGroupEcho("toms"));
$("vfx-clear").addEventListener("click", () => {
  checkpoint("vfx-clear");
  clearAllVoiceFx(voiceFx);
  commit();
});

flamModeBtn.addEventListener("click", () => {
  flamMode = !flamMode;
  renderControls();
});

presetSelect.addEventListener("change", () => {
  const preset = PRESETS.find((p) => p.id === presetSelect.value);
  if (!preset) {
    presetSelect.value = activePresetId;
    return;
  }
  checkpoint();
  const next = preset.spec();
  next.render = { ...spec.render }; // the booth's master settings stay
  spec = next;
  activePresetId = preset.id;
  if (preset.kitId) activeKitId = preset.kitId;
  commit();
});

kitSelect.addEventListener("change", () => {
  const kit = KITS.find((k) => k.id === kitSelect.value);
  if (!kit) {
    kitSelect.value = activeKitId;
    return;
  }
  checkpoint();
  spec = applyKitToSpec(spec, kit);
  activeKitId = kit.id;
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

// The skip link moves focus into the grid without touching location.hash
// (a "#grid" fragment would replace the #p= pattern in the address bar).
app.querySelector<HTMLAnchorElement>(".skip")?.addEventListener("click", (e) => {
  e.preventDefault();
  app.querySelector<HTMLButtonElement>('#grid .step[tabindex="0"]')?.focus();
});

function isTextEntry(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement) return true;
  if (t instanceof HTMLInputElement) return !["range", "button", "submit", "reset"].includes(t.type);
  return false;
}

const VOICE_DIGIT_CODES = [
  "Digit1",
  "Digit2",
  "Digit3",
  "Digit4",
  "Digit5",
  "Digit6",
  "Digit7",
  "Digit8",
  "Digit9",
  "Digit0",
] as const;

const STEP_KEY_CODES = [
  "KeyA",
  "KeyS",
  "KeyD",
  "KeyF",
  "KeyG",
  "KeyH",
  "KeyJ",
  "KeyK",
  "KeyZ",
  "KeyX",
  "KeyC",
  "KeyV",
  "KeyB",
  "KeyN",
  "KeyM",
  "Comma",
] as const;

const KIT_KEY_CODES = ["KeyY", "KeyU", "KeyI", "KeyO", "KeyP"] as const;
const RMX_ROLL_DIVS = [1, 2, 4, 8, 16, 32] as const;

function applyPresetByIndex(idx: number): void {
  const wrapped = ((idx % PRESETS.length) + PRESETS.length) % PRESETS.length;
  const preset = PRESETS[wrapped];
  if (!preset) return;
  checkpoint();
  const next = preset.spec();
  next.render = { ...spec.render };
  spec = next;
  activePresetId = preset.id;
  if (preset.kitId) activeKitId = preset.kitId;
  commit();
}

window.addEventListener("keydown", (e) => {
  const mod = e.ctrlKey || e.metaKey;
  if (e.key === "Escape") {
    if (howTo.isOpen) {
      e.preventDefault();
      howTo.close();
      return;
    }
    rmx.setReleaseLever(false);
    return;
  }
  if (isTextEntry(e.target)) return;

  if (e.code === "Slash" && !mod && !e.altKey) {
    e.preventDefault();
    if (!e.repeat) howTo.toggle(surfaceMode);
    return;
  }
  if (e.code === "Space" && !mod && !e.altKey) {
    // Space is play/stop everywhere, also on a focused button (Enter
    // activates buttons).
    e.preventDefault();
    if (!e.repeat) {
      howTo.notifyKeyDown("Space", "Space → Play / Stop Transport");
      void togglePlay();
    }
    return;
  }
  if (mod && !e.altKey && e.key.toLowerCase() === "z") {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
    return;
  }
  if (mod && !e.altKey && e.key.toLowerCase() === "y") {
    e.preventDefault();
    redo();
    return;
  }

  // Ctrl + 1..0: Toggle Dub Echo send on Voice 1..0 directly
  const vIdx = VOICE_DIGIT_CODES.indexOf(e.code as (typeof VOICE_DIGIT_CODES)[number]);
  if (e.ctrlKey && !e.metaKey && !e.altKey && vIdx >= 0 && !e.repeat) {
    const vInfo = VOICES[vIdx]!;
    e.preventDefault();
    toggleVoiceFx(vInfo.id, "echo");
    const on = voiceFx[vInfo.id].echo;
    howTo.notifyKeyDown(
      e.code,
      `Ctrl+${vIdx === 9 ? 0 : vIdx + 1} → ${vInfo.short} Dub Echo ${on ? "ON" : "OFF"}`,
    );
    return;
  }

  if (mod) return;

  // 1. Number Row 1..0: Select Voice, Shift+1..0 Live Drum Trigger, Option+1..0 Mute
  if (vIdx >= 0 && !e.repeat) {
    const vInfo = VOICES[vIdx]!;
    e.preventDefault();
    if (e.altKey) {
      checkpoint();
      spec.voices[vInfo.id].mute = !spec.voices[vInfo.id].mute;
      commit();
      howTo.notifyKeyDown(e.code, `⌥+${vIdx === 9 ? 0 : vIdx + 1} → Mute ${vInfo.short}`);
      return;
    }
    if (e.shiftKey) {
      selected = vInfo.id;
      rmx.triggerVoicePad(vInfo.id, true);
      renderVoicePanel();
      renderGrid();
      renderGrooveAndFxControls();
      howTo.notifyKeyDown(e.code, `Shift+${vIdx === 9 ? 0 : vIdx + 1} → Trigger ${vInfo.short}`);
      return;
    }
    selected = vInfo.id;
    grid.setFocus(vIdx, 0, false);
    renderVoicePanel();
    renderGrid();
    renderGrooveAndFxControls();
    if (surfaceMode === "rmx") {
      if (vIdx < 4) {
        rmx.triggerPadSlot(vIdx);
      } else {
        const div = RMX_ROLL_DIVS[vIdx - 4] ?? 8;
        rmx.holdRoll(div);
      }
    }
    howTo.notifyKeyDown(e.code, `${vIdx === 9 ? 0 : vIdx + 1} → Select ${vInfo.short} (${vInfo.name})`);
    return;
  }

  // Option (Alt) shortcuts for Groove / Variation & Selected-Voice FX
  if (e.altKey && !e.repeat) {
    switch (e.code) {
      case "KeyG":
        e.preventDefault();
        triggerHumanizeAction();
        howTo.notifyKeyDown(e.code, `⌥+G → Humanize Pattern (${pct(humanizeAmount)})`);
        return;
      case "KeyV":
        e.preventDefault();
        triggerMutateAction();
        howTo.notifyKeyDown(e.code, "⌥+V → Pattern Variation");
        return;
      case "KeyF":
        e.preventDefault();
        triggerAutoFillAction();
        howTo.notifyKeyDown(e.code, "⌥+F → 1-Bar Auto-Fill (Steps 13–16)");
        return;
      case "KeyE":
        e.preventDefault();
        toggleVoiceFx(selected, "echo");
        howTo.notifyKeyDown(
          e.code,
          `⌥+E → ${selected.toUpperCase()} Dub Echo ${voiceFx[selected].echo ? "ON" : "OFF"}`,
        );
        return;
      case "KeyR":
        e.preventDefault();
        toggleVoiceFx(selected, "reverb");
        howTo.notifyKeyDown(
          e.code,
          `⌥+R → ${selected.toUpperCase()} Space Reverb ${voiceFx[selected].reverb ? "ON" : "OFF"}`,
        );
        return;
      case "KeyD":
        e.preventDefault();
        toggleVoiceFx(selected, "filter");
        howTo.notifyKeyDown(
          e.code,
          `⌥+D → ${selected.toUpperCase()} Acid Filter ${voiceFx[selected].filter ? "ON" : "OFF"}`,
        );
        return;
      case "KeyX":
        e.preventDefault();
        checkpoint("vfx-clear");
        clearAllVoiceFx(voiceFx);
        commit();
        howTo.notifyKeyDown(e.code, "⌥+X → All Per-Voice FX Cleared (Dry)");
        return;
    }
    return;
  }

  // 2. Home Row (A..K) + Bottom Row (Z..,) -> Steps 1..16
  const stepIdx = STEP_KEY_CODES.indexOf(e.code as (typeof STEP_KEY_CODES)[number]);
  if (stepIdx >= 0 && !e.repeat) {
    e.preventDefault();
    checkpoint();
    const v = spec.voices[selected];
    v.steps = e.shiftKey || flamMode ? toggleFlam(v.steps, stepIdx) : cycleStep(v.steps, stepIdx);
    const rIdx = Math.max(
      0,
      VOICES.findIndex((item) => item.id === selected),
    );
    grid.setFocus(rIdx, stepIdx, false);
    commit();
    howTo.notifyKeyDown(
      e.code,
      `${e.shiftKey ? "Shift+" : ""}Step ${stepIdx + 1} (${selected.toUpperCase()})`,
    );
    return;
  }

  // 3. Kit Voicing Keys Y U I O P -> 5 Analog Drum Machine Kits
  const kitIdx = KIT_KEY_CODES.indexOf(e.code as (typeof KIT_KEY_CODES)[number]);
  if (kitIdx >= 0 && !e.repeat) {
    const kit = KITS[kitIdx];
    if (kit) {
      e.preventDefault();
      checkpoint();
      spec = applyKitToSpec(spec, kit);
      activeKitId = kit.id;
      commit();
      howTo.notifyKeyDown(e.code, `Kit → ${kit.name}`);
    }
    return;
  }

  // 4. Surface Mode, Presets, Group FX (Shift+Q/W/E), Flam, RMX Release FX, Tempo, Nudge & Clear
  switch (e.code) {
    case "Backquote":
      if (!e.repeat) {
        e.preventDefault();
        setSurfaceMode(surfaceMode === "rmx" ? "sequencer" : "rmx");
        howTo.notifyKeyDown(e.code, `Surface → ${surfaceMode === "rmx" ? "RMX-1000" : "Step Sequencer"}`);
      }
      break;
    case "KeyQ":
      if (!e.repeat) {
        e.preventDefault();
        if (surfaceMode === "rmx") {
          rmx.triggerPadSlot(0);
          howTo.notifyKeyDown(e.code, "Q → X-Pad Slot 1");
        } else if (e.shiftKey) {
          const on = triggerGroupEcho("snare_clap");
          howTo.notifyKeyDown(e.code, `Shift+Q → Snare+Clap+Rim Echo ${on ? "ON" : "OFF"}`);
        } else {
          const cur = PRESETS.findIndex((p) => p.id === app.dataset.preset);
          applyPresetByIndex(cur <= 0 ? PRESETS.length - 1 : cur - 1);
          howTo.notifyKeyDown(e.code, `Q → Preset: ${app.dataset.preset}`);
        }
      }
      break;
    case "KeyW":
      if (!e.repeat) {
        e.preventDefault();
        if (surfaceMode === "rmx") {
          rmx.triggerPadSlot(1);
          howTo.notifyKeyDown(e.code, "W → X-Pad Slot 2");
        } else if (e.shiftKey) {
          const on = triggerGroupEcho("hats");
          howTo.notifyKeyDown(e.code, `Shift+W → Hi-Hats Echo ${on ? "ON" : "OFF"}`);
        } else {
          const cur = PRESETS.findIndex((p) => p.id === app.dataset.preset);
          applyPresetByIndex(cur < 0 ? 1 : cur + 1);
          howTo.notifyKeyDown(e.code, `W → Preset: ${app.dataset.preset}`);
        }
      }
      break;
    case "KeyE":
      if (!e.repeat) {
        e.preventDefault();
        if (surfaceMode === "rmx") {
          rmx.triggerPadSlot(2);
          howTo.notifyKeyDown(e.code, "E → X-Pad Slot 3");
        } else if (e.shiftKey) {
          const on = triggerGroupEcho("toms");
          howTo.notifyKeyDown(e.code, `Shift+E → Toms+Cowbell Echo ${on ? "ON" : "OFF"}`);
        } else {
          flamMode = !flamMode;
          renderControls();
          howTo.notifyKeyDown(e.code, `E → Flam Edit ${flamMode ? "ON" : "OFF"}`);
        }
      }
      break;
    case "KeyR":
      if (!e.repeat) {
        e.preventDefault();
        if (surfaceMode === "rmx" && e.shiftKey) {
          rmx.triggerPadSlot(3);
          howTo.notifyKeyDown(e.code, "Shift+R → X-Pad Slot 4");
        } else {
          rmx.setReleaseLever(true);
          howTo.notifyKeyDown(e.code, "Hold R → RMX-1000 Release FX Engaged");
        }
      }
      break;
    case "KeyT":
      if (!e.repeat) {
        tap(e.timeStamp);
        howTo.notifyKeyDown(e.code, "T → Tap Tempo");
      }
      break;
    case "BracketLeft":
    case "Minus":
      if (!clock.following) {
        e.preventDefault();
        setBpm(e.shiftKey ? spec.bpm - 0.1 : Math.round(spec.bpm) - 1, "bpm");
        howTo.notifyKeyDown(e.code, `[ → BPM ${spec.bpm}`);
      }
      break;
    case "BracketRight":
    case "Equal":
      if (!clock.following) {
        e.preventDefault();
        setBpm(e.shiftKey ? spec.bpm + 0.1 : Math.round(spec.bpm) + 1, "bpm");
        howTo.notifyKeyDown(e.code, `] → BPM ${spec.bpm}`);
      }
      break;
    case "Semicolon":
      e.preventDefault();
      $<HTMLButtonElement>("nudge-down").dispatchEvent(
        new MouseEvent("click", { shiftKey: e.shiftKey }),
      );
      howTo.notifyKeyDown(e.code, "; → Phase Nudge Earlier");
      break;
    case "Quote":
      e.preventDefault();
      $<HTMLButtonElement>("nudge-up").dispatchEvent(
        new MouseEvent("click", { shiftKey: e.shiftKey }),
      );
      howTo.notifyKeyDown(e.code, "' → Phase Nudge Later");
      break;
    case "KeyL":
      if (!e.repeat) {
        e.preventDefault();
        $<HTMLButtonElement>("resync").click();
        howTo.notifyKeyDown(e.code, "L → Re-sync Bar Phase");
      }
      break;
    case "Period":
      if (!e.repeat) {
        e.preventDefault();
        checkpoint();
        spec.voices[selected].mute = !spec.voices[selected].mute;
        commit();
        howTo.notifyKeyDown(e.code, `. → Mute ${selected.toUpperCase()}`);
      }
      break;
    case "Backspace":
      if (!e.repeat) {
        e.preventDefault();
        checkpoint();
        if (e.shiftKey) {
          for (const v of VOICES) spec.voices[v.id].steps = EMPTY_STEPS;
          howTo.notifyKeyDown(e.code, "Shift+Delete → Clear All Voices");
        } else {
          spec.voices[selected].steps = EMPTY_STEPS;
          howTo.notifyKeyDown(e.code, `Delete → Clear ${selected.toUpperCase()}`);
        }
        commit();
      }
      break;
  }
});
window.addEventListener("pagehide", () => {
  if (urlTimer !== undefined) writeUrl();
});
window.addEventListener("keyup", (e) => {
  if (e.code === "Space" && e.target instanceof HTMLButtonElement) e.preventDefault();
  if (e.code === "KeyR") rmx.setReleaseLever(false);
  if (
    ["Digit5", "Digit6", "Digit7", "Digit8", "Digit9", "Digit0"].includes(e.code) &&
    surfaceMode === "rmx"
  ) {
    rmx.releaseRoll();
  }
  howTo.notifyKeyUp(e.code);
});

// ---- boot ----

if (!SINGLE) setupPwa($<HTMLButtonElement>("install"));
audio.prepare(); // compile the core now; audio itself waits for Play
commit();
renderPlayhead();
renderStatus("idle");
