// Static markup of the instrument. Dynamic parts (grid rows, voice
// controls, device lists) are filled in by the modules that own them.

import { PRESETS } from "../presets";

export interface TemplateOptions {
  single: boolean;
  bridge: boolean;
  midi: boolean;
}

const knob = (id: string, label: string, min = 0, max = 1, step = 0.01) => `
  <label class="knob" for="${id}">
    <span class="knob-label">${label} <output id="${id}-out" for="${id}"></output></span>
    <input id="${id}" type="range" min="${min}" max="${max}" step="${step}" />
  </label>`;

export function template(o: TemplateOptions): string {
  const presetOptions = PRESETS.map(
    (p) => `<option value="${p.id}" title="${p.blurb}">${p.name}</option>`,
  ).join("");
  const unavailable = o.single
    ? "Bridge and MIDI clock need the full app; they are switched off in this embedded copy."
    : !o.bridge && !o.midi
      ? "Bridge and MIDI clock are not available in this browser."
      : !o.midi
        ? "MIDI clock needs a browser with Web MIDI (Chrome, Edge)."
        : "";
  return `
  <a class="skip" href="#grid">Skip to the pattern</a>
  <header class="top">
    <h1 class="logo">player<span>5</span></h1>
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
      <button id="undo" type="button" aria-keyshortcuts="Control+Z Meta+Z" title="Undo (Ctrl/Cmd+Z)">Undo</button>
      <button id="clear" type="button" title="Clear every step (undo restores)">Clear</button>
      <button id="share" type="button" title="${o.single ? "Copy the pattern code" : "Copy a link to this pattern"}">Share</button>
    </div>
  </section>

  <section class="panel grid-panel" aria-labelledby="grid-title">
    <div class="panel-head">
      <h2 id="grid-title">Pattern</h2>
      <span class="hint" id="grid-hint"></span>
    </div>
    <div id="grid" class="grid" tabindex="-1"></div>
  </section>

  <div class="split">
    <section class="panel voice-panel" aria-labelledby="voice-title">
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
    <p id="source-status" class="source-status" aria-live="polite"></p>

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
