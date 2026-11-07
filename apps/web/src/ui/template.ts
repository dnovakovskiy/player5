// Static markup of the instrument. Dynamic parts (grid rows, voice
// controls, device lists) are filled in by the modules that own them.

import { KITS, PRESETS, type PresetCategory } from "../presets";

export interface TemplateOptions {
  single: boolean;
  bridge: boolean;
  midi: boolean;
  /** window.isSecureContext: Web MIDI (and the AudioWorklet) need it. */
  secure: boolean;
}

const knob = (id: string, label: string, min = 0, max = 1, step = 0.01) => `
  <label class="knob" for="${id}">
    <span class="knob-label">${label} <output id="${id}-out" for="${id}"></output></span>
    <input id="${id}" type="range" min="${min}" max="${max}" step="${step}" />
  </label>`;

export function template(o: TemplateOptions): string {
  const categories: PresetCategory[] = ["Core Booth", "Nu-Disco", "House", "Techno"];
  const presetOptions = categories
    .map((cat) => {
      const items = PRESETS.filter((p) => p.category === cat)
        .map((p) => `<option value="${p.id}" title="${p.blurb}">${p.name}</option>`)
        .join("");
      return `<optgroup label="${cat}">${items}</optgroup>`;
    })
    .join("");
  const kitOptions = KITS.map(
    (k) => `<option value="${k.id}" title="${k.blurb}">${k.name}</option>`,
  ).join("");
  const unavailable = o.single
    ? "Bridge and MIDI clock need the full app; they are switched off in this embedded copy."
    : !o.bridge && !o.midi
      ? "Bridge and MIDI clock are not available in this browser."
      : !o.midi
        ? o.secure
          ? "MIDI clock needs a browser with Web MIDI (Chrome, Edge)."
          : "MIDI clock needs a secure page: open the app over https or from localhost."
        : "";
  return `
  <a class="skip" href="#grid">Skip to the pattern</a>
  <header class="top">
    <h1 class="logo">player<span>5</span></h1>
    <div class="mode-tabs" role="group" aria-label="Surface mode">
      <button id="mode-seq" type="button" class="mode-tab" aria-pressed="true">Step Sequencer</button>
      <button id="mode-rmx" type="button" class="mode-tab" aria-pressed="false">RMX-1000 Station</button>
    </div>
    <div class="top-status">
      <span id="engine-mode" class="engine-mode" title="Audio engine runtime"></span>
      <span id="status" class="status" role="status" aria-live="polite">audio off</span>
    </div>
    <button id="install" type="button" class="install" hidden>Install</button>
  </header>

  <section class="panel transport" aria-label="Transport">
    <button id="play" class="play" type="button" aria-pressed="false" aria-keyshortcuts="Space">Play</button>
    <div class="bpm" role="group" aria-labelledby="bpm-label">
      <span class="label" id="bpm-label">BPM</span>
      <div class="bpm-row">
        <button type="button" class="sq" data-bpm="-1" aria-label="Tempo down" title="Tempo down 1 BPM (Shift: 0.1)">−</button>
        <input id="bpm" type="number" inputmode="decimal" min="20" max="400" step="0.01" aria-labelledby="bpm-label" />
        <button type="button" class="sq" data-bpm="1" aria-label="Tempo up" title="Tempo up 1 BPM (Shift: 0.1)">+</button>
        <button type="button" id="tap" class="tap" aria-keyshortcuts="T" title="Tap tempo (T)">Tap</button>
      </div>
    </div>
    <div class="feel">
      ${knob("shuffle", "Shuffle")}
      ${knob("accent", "Accent")}
      ${knob("flam", "Flam")}
    </div>
    <div class="tools">
      <button id="flam-mode" type="button" class="toggle" aria-pressed="false" title="While on, tapping a step toggles its flam">Flam edit</button>
      <select id="preset" aria-label="Load a preset pattern">
        <option value="">Presets…</option>${presetOptions}
      </select>
      <select id="kit" aria-label="Load a drum kit voicing" title="Switch analog drum machine voicing without changing the step pattern">
        <option value="">Drum kit…</option>${kitOptions}
      </select>
      <button id="undo" type="button" aria-keyshortcuts="Control+Z Meta+Z" title="Undo (Ctrl/Cmd+Z)">Undo</button>
      <button id="clear" type="button" title="Clear every step (undo restores)">Clear</button>
      <button id="share" type="button" title="${o.single ? "Copy the pattern code" : "Copy a link to this pattern"}">Share</button>
    </div>
  </section>

  <section class="panel rmx-panel" id="rmx-panel" aria-labelledby="rmx-title" hidden>
    <div class="panel-head rmx-head">
      <div class="rmx-brand">
        <h2 id="rmx-title">RMX-1000 · Remix Station</h2>
        <output id="rmx-readout" class="rmx-readout" aria-live="polite">BPF ECHO 0% · X-PAD SNARE 1/8</output>
      </div>
      <button id="rmx-reset" type="button" class="small" title="Reset all RMX-1000 FX parameters">Reset FX</button>
    </div>

    <div class="rmx-grid">
      <div class="rmx-box rmx-iso" role="group" aria-labelledby="rmx-iso-heading">
        <div class="rmx-box-head">
          <h3 id="rmx-iso-heading">ISOLATOR FX</h3>
          <span class="hint">3-band EQ &amp; rhythmic gate</span>
        </div>
        <div class="rmx-modes" role="group" aria-label="Isolator FX mode">
          <button type="button" data-iso-mode="isolator" aria-pressed="true">ISOLATOR</button>
          <button type="button" data-iso-mode="cut_add" aria-pressed="false">CUT/ADD</button>
          <button type="button" data-iso-mode="trans_roll" aria-pressed="false">TRANS/ROLL</button>
          <button type="button" data-iso-mode="gate_drive" aria-pressed="false">GATE/DRIVE</button>
        </div>
        <div class="rmx-iso-bands">
          <div class="rmx-band">
            <label class="knob" for="rmx-iso-low">
              <span class="knob-label">LOW <output id="rmx-iso-low-out" for="rmx-iso-low">0 dB</output></span>
              <input id="rmx-iso-low" type="range" min="-48" max="6" step="1" value="0" />
            </label>
            <button id="rmx-kill-low" type="button" class="toggle small" aria-pressed="false">KILL LOW</button>
          </div>
          <div class="rmx-band">
            <label class="knob" for="rmx-iso-mid">
              <span class="knob-label">MID <output id="rmx-iso-mid-out" for="rmx-iso-mid">0 dB</output></span>
              <input id="rmx-iso-mid" type="range" min="-48" max="6" step="1" value="0" />
            </label>
            <button id="rmx-kill-mid" type="button" class="toggle small" aria-pressed="false">KILL MID</button>
          </div>
          <div class="rmx-band">
            <label class="knob" for="rmx-iso-high">
              <span class="knob-label">HIGH <output id="rmx-iso-high-out" for="rmx-iso-high">0 dB</output></span>
              <input id="rmx-iso-high" type="range" min="-48" max="6" step="1" value="0" />
            </label>
            <button id="rmx-kill-high" type="button" class="toggle small" aria-pressed="false">KILL HIGH</button>
          </div>
        </div>
      </div>

      <div class="rmx-box rmx-xpad" role="group" aria-labelledby="rmx-xpad-heading">
        <div class="rmx-box-head">
          <h3 id="rmx-xpad-heading">X-PAD FX</h3>
          <button id="rmx-bank" type="button" class="toggle small" aria-pressed="false">Bank A (BD/SD/CP/CH)</button>
        </div>
        <div class="rmx-pads" role="group" aria-label="X-Pad drum voices">
          <button type="button" class="rmx-pad-btn" data-xpad-slot="0" aria-pressed="false">BD · KICK</button>
          <button type="button" class="rmx-pad-btn" data-xpad-slot="1" aria-pressed="true">SD · SNARE</button>
          <button type="button" class="rmx-pad-btn" data-xpad-slot="2" aria-pressed="false">CP · CLAP</button>
          <button type="button" class="rmx-pad-btn" data-xpad-slot="3" aria-pressed="false">CH · HI-HAT</button>
        </div>
        <div class="rmx-strip-wrap">
          <span class="label">TOUCH ROLL RIBBON (HOLD OR SLIDE)</span>
          <div id="rmx-xpad-strip" class="rmx-strip" role="group" aria-label="X-Pad beat roll division">
            <button type="button" data-roll-div="1" aria-pressed="false">1/1</button>
            <button type="button" data-roll-div="2" aria-pressed="false">1/2</button>
            <button type="button" data-roll-div="4" aria-pressed="false">1/4</button>
            <button type="button" data-roll-div="8" aria-pressed="true">1/8</button>
            <button type="button" data-roll-div="16" aria-pressed="false">1/16</button>
            <button type="button" data-roll-div="32" aria-pressed="false">1/32</button>
          </div>
        </div>
        <div class="rmx-xpad-controls">
          <label class="knob" for="rmx-pitch">
            <span class="knob-label">PITCH <output id="rmx-pitch-out" for="rmx-pitch">0 st</output></span>
            <input id="rmx-pitch" type="range" min="-1" max="1" step="0.05" value="0" />
          </label>
          <div class="rmx-xpad-actions">
            <button id="rmx-roll-latch" type="button" class="toggle small" aria-pressed="false" title="Hold roll continuously">LATCH ROLL</button>
            <button id="rmx-overdub" type="button" class="toggle small" aria-pressed="false" title="Record X-Pad hits into the 16-step pattern">OVERDUB</button>
            <button id="rmx-voice-mute" type="button" class="toggle small" aria-pressed="false" title="Silence selected X-Pad voice in pattern">PAD MUTE</button>
            <button id="rmx-voice-clear" type="button" class="small" title="Erase selected X-Pad voice steps">ERASE PAD</button>
          </div>
        </div>
      </div>

      <div class="rmx-box rmx-scene" role="group" aria-labelledby="rmx-scene-heading">
        <div class="rmx-box-head">
          <h3 id="rmx-scene-heading">SCENE FX</h3>
          <span class="hint">Build Up &amp; Break Down</span>
        </div>
        <div class="rmx-scene-banks">
          <div class="rmx-scene-col">
            <span class="label rmx-build-label">BUILD UP</span>
            <button type="button" class="rmx-fx-btn build" data-scene-fx="bpf_echo" aria-pressed="true">BPF ECHO</button>
            <button type="button" class="rmx-fx-btn build" data-scene-fx="echo" aria-pressed="false">ECHO</button>
            <button type="button" class="rmx-fx-btn build" data-scene-fx="noise" aria-pressed="false">NOISE</button>
            <button type="button" class="rmx-fx-btn build" data-scene-fx="spiral_up" aria-pressed="false">SPIRAL UP</button>
            <button type="button" class="rmx-fx-btn build" data-scene-fx="reverb_up" aria-pressed="false">REVERB UP</button>
          </div>
          <div class="rmx-scene-col">
            <span class="label rmx-break-label">BREAK DOWN</span>
            <button type="button" class="rmx-fx-btn break" data-scene-fx="hpf_echo" aria-pressed="false">HPF ECHO</button>
            <button type="button" class="rmx-fx-btn break" data-scene-fx="lpf_echo" aria-pressed="false">LPF ECHO</button>
            <button type="button" class="rmx-fx-btn break" data-scene-fx="crush_echo" aria-pressed="false">CRUSH ECHO</button>
            <button type="button" class="rmx-fx-btn break" data-scene-fx="spiral_down" aria-pressed="false">SPIRAL DOWN</button>
            <button type="button" class="rmx-fx-btn break" data-scene-fx="reverb_down" aria-pressed="false">REVERB DOWN</button>
          </div>
        </div>
        <div class="rmx-scene-knobs">
          <label class="knob" for="rmx-scene-amount">
            <span class="knob-label">SCENE LEVEL / TIME <output id="rmx-scene-amount-out" for="rmx-scene-amount">0%</output></span>
            <input id="rmx-scene-amount" type="range" min="0" max="1" step="0.01" value="0" />
          </label>
          <label class="knob" for="rmx-scene-sub">
            <span class="knob-label">SUB PARAM (RESO / FB) <output id="rmx-scene-sub-out" for="rmx-scene-sub">50%</output></span>
            <input id="rmx-scene-sub" type="range" min="0" max="1" step="0.01" value="0.5" />
          </label>
        </div>
      </div>
    </div>

    <div class="rmx-release" role="group" aria-labelledby="rmx-release-heading">
      <div class="rmx-release-modes">
        <span class="label" id="rmx-release-heading">RELEASE FX</span>
        <button type="button" data-release-mode="vinyl_brake" aria-pressed="true">VINYL BRAKE</button>
        <button type="button" data-release-mode="echo" aria-pressed="false">ECHO</button>
        <button type="button" data-release-mode="back_spin" aria-pressed="false">BACK SPIN</button>
      </div>
      <button id="rmx-release-lever" type="button" class="rmx-lever" aria-pressed="false" title="Hold to engage Release FX and clear Scene FX on release">
        PULL RELEASE LEVER (HOLD)
      </button>
    </div>
  </section>

  <section class="panel grid-panel" id="grid-panel" aria-labelledby="grid-title">
    <div class="panel-head">
      <h2 id="grid-title">Pattern</h2>
      <span class="hint" id="grid-hint"></span>
    </div>
    <div id="grid" class="grid" tabindex="-1"></div>
  </section>

  <div class="split">
    <section class="panel voice-panel" id="voice-panel" aria-labelledby="voice-title">
      <div class="panel-head">
        <h2 id="voice-title">Voice</h2>
        <span class="hint">select a row label</span>
      </div>
      <div id="voice-controls" class="knobs"></div>
    </section>

    <section class="panel master" aria-labelledby="master-title">
      <div class="panel-head"><h2 id="master-title">Master</h2></div>
      ${knob("gain", "Output", -24, 12, 0.5)}
      <label class="switch" for="limiter">
        <input id="limiter" type="checkbox" role="switch" />
        <span>Safety limiter</span>
      </label>
      <div class="meter-wrap">
        <div id="meter" class="meter" role="meter" aria-label="Output peak level" aria-valuemin="-48" aria-valuemax="0" aria-valuenow="-48" aria-valuetext="silent" data-peak-db="-inf">
          <div class="meter-fill" id="meter-fill"></div>
          <div class="meter-mark" title="−6 dBFS: default peak level"></div>
        </div>
        <div class="meter-scale" aria-hidden="true"><span>−48</span><span>−24</span><span class="m6">−6</span><span>0</span></div>
        <output id="meter-out" class="meter-out">−∞ dBFS</output>
      </div>
    </section>
  </div>

  <section class="panel clock" id="clock" aria-labelledby="clock-title" data-source="internal" data-lock="off">
    <div class="panel-head">
      <h2 id="clock-title">Clock</h2>
      <div class="seg" role="radiogroup" aria-label="Clock source">
        <label><input type="radio" name="clock-source" value="internal" checked /><span>Internal</span></label>
        <label><input type="radio" name="clock-source" value="tap" /><span>Tap</span></label>
        <label${o.bridge ? "" : " hidden"}><input type="radio" name="clock-source" value="bridge"${o.bridge ? "" : " disabled"} /><span>Bridge</span></label>
        <label${o.midi ? "" : " hidden"}><input type="radio" name="clock-source" value="midi"${o.midi ? "" : " disabled"} /><span>MIDI</span></label>
      </div>
    </div>
    ${unavailable ? `<p class="hint unavailable" id="clock-unavailable">${unavailable}</p>` : ""}
    <div class="readout">
      <div class="cell"><span class="label">Tempo</span><output id="clock-tempo" class="big">—</output></div>
      <div class="cell"><span class="label">Lock</span><output id="clock-lock" class="big">—</output></div>
      <div class="cell"><span class="label">Bar</span>
        <span class="beats" aria-hidden="true"><i></i><i></i><i></i><i></i></span>
        <output id="clock-beat" class="beat-num" aria-label="Beat in bar">–</output>
      </div>
    </div>
    <p class="source-status"><span id="source-status" aria-live="polite"></span><span id="source-detail" class="source-detail"></span></p>

    <div class="source-panel" data-for="tap">
      <button id="tap-pad" type="button" class="tap-pad" aria-keyshortcuts="T">Tap the beat</button>
    </div>
    <div class="source-panel" data-for="bridge">
      <div class="row-fields">
        <label class="field grow" for="bridge-url"><span class="label">Bridge URL</span>
          <input id="bridge-url" type="text" spellcheck="false" autocomplete="off" inputmode="url" /></label>
        <button id="bridge-connect" type="button">Connect</button>
      </div>
      <div class="row-fields">
        <label class="field" for="bridge-follow"><span class="label">Follow</span>
          <select id="bridge-follow"><option value="master">Tempo master</option></select></label>
      </div>
      <ul id="bridge-devices" class="devices" aria-label="Devices on the network"></ul>
    </div>
    <div class="source-panel" data-for="midi">
      <label class="field" for="midi-input"><span class="label">MIDI input</span>
        <select id="midi-input"><option value="">—</option></select></label>
    </div>

    <div class="clock-controls">
      <div class="field" role="group" aria-labelledby="nudge-label">
        <span class="label" id="nudge-label">Phase nudge</span>
        <div class="inline">
          <button type="button" class="sq" id="nudge-down" aria-label="Nudge earlier" title="1 ms earlier (Shift: 10 ms)">−</button>
          <output id="nudge-out" class="mono-out">0 ms</output>
          <button type="button" class="sq" id="nudge-up" aria-label="Nudge later" title="1 ms later (Shift: 10 ms)">+</button>
          <button type="button" id="nudge-reset" class="small" title="Reset nudge">0</button>
        </div>
      </div>
      <label class="field" for="latency"><span class="label">Latency offset (ms)</span>
        <input id="latency" type="number" step="1" min="-250" max="250" inputmode="numeric" /></label>
      <button id="resync" type="button" title="Snap to the source (internal: restart the bar)">Re-sync</button>
    </div>
  </section>

  <section class="panel share" id="share-panel" aria-labelledby="share-title">
    <div class="panel-head"><h2 id="share-title">Share &amp; import</h2></div>
    <div class="share-row"${o.single ? " hidden" : ""}>
      <label for="share-link">Link</label>
      <input id="share-link" type="text" readonly spellcheck="false" />
      <button id="copy-link" type="button">Copy</button>
    </div>
    <div class="share-row">
      <label for="share-code">Pattern code</label>
      <input id="share-code" type="text" readonly spellcheck="false" />
      <button id="copy-code" type="button">Copy</button>
    </div>
    <div class="share-row">
      <label for="import">Import</label>
      <input id="import" type="text" spellcheck="false" autocomplete="off" placeholder="paste a pattern code or link" />
      <button id="import-btn" type="button">Load</button>
    </div>
    <p id="share-msg" class="hint" role="status" aria-live="polite"></p>
  </section>

  <footer class="foot">
    <span>Space play/stop · Enter toggles a step · arrows move · T tap · Ctrl/Cmd+Z undo</span>
    <span>TR-inspired drum machine for the DJ booth</span>
  </footer>`;
}
