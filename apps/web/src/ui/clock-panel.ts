// The clock panel: source selection (Internal, Tap, Ableton Link, Bridge, MIDI),
// engine tempo / lock / bar position readouts, and the global timing controls
// (phase nudge, latency offset, re-sync). Timed events from the sources
// are mapped onto the engine's sample clock by AudioEngine (TimeMap) and
// applied inside the audio runtime between blocks.

import { BridgeClient, DEFAULT_BRIDGE_URL, discoverBridgeUrl, PRECISION_MODE } from "../clock/bridge";
import type { BridgeDevice, BridgeTimeline } from "../clock/bridge";
import { MidiClockInput, type MidiPort } from "../clock/midi";
import type { AudioEngine, AudioState, EngineStatus } from "../engine/audio-engine";
import { loadNumber, loadString, saveString } from "../storage";

export type SourceKind = "internal" | "tap" | "link" | "bridge" | "midi";

export interface ClockPanelHost {
  audio: AudioEngine;
  /** A tap at performance time `ms` (tempo estimate + engine phase). */
  tap(ms: number): void;
  /** Called after the source changed. */
  onSourceChange(source: SourceKind, previous: SourceKind): void;
  /** Current pattern BPM (used to seed a standalone Ableton Link session). */
  getPatternBpm?(): number;
}

export interface ClockPanelOptions {
  bridge: boolean;
  midi: boolean;
}

const $ = <T extends HTMLElement>(root: ParentNode, sel: string) => root.querySelector(sel) as T;

export class ClockPanel {
  source: SourceKind = "internal";
  private nudgeMs = 0;
  private latencyMs: number;
  private bridge: BridgeClient | null = null;
  private bridgeUrl: string;
  private bridgeUrlFromUser: boolean;
  private discovered = false;
  private timeline: BridgeTimeline | null = null;
  private bridgeState = "closed";
  private bridgeStatus = "";
  /** What the Follow menu asks the bridge for; survives device-list changes. */
  private followTarget: "master" | number = "master";
  private midi: MidiClockInput | null = null;
  private midiPorts: MidiPort[] = [];
  private midiError = "";
  private audioState: AudioState = "idle";

  // Local browser-tab Ableton Link fallback mesh (used while bridge is offline)
  private linkChannel: BroadcastChannel | null = null;
  private linkTimer: ReturnType<typeof setInterval> | null = null;
  private linkPeerId = Math.random().toString(36).slice(2, 10);
  private linkPeers = new Map<string, number>();
  private linkAnchorEpochMs = Date.now();
  private linkAnchorBeat = 0;
  private linkBpm = 124;

  private readonly el: {
    root: HTMLElement;
    tempo: HTMLOutputElement;
    lock: HTMLOutputElement;
    beat: HTMLOutputElement;
    beats: HTMLElement[];
    status: HTMLElement;
    statusDetail: HTMLElement;
    nudge: HTMLOutputElement;
    latency: HTMLInputElement;
    bridgeUrl: HTMLInputElement;
    bridgeFollow: HTMLSelectElement;
    devices: HTMLUListElement;
    linkUrl: HTMLInputElement | null;
    linkPeersOut: HTMLElement | null;
    midiInput: HTMLSelectElement;
  };

  constructor(
    root: HTMLElement,
    private readonly host: ClockPanelHost,
    private readonly opts: ClockPanelOptions,
  ) {
    this.el = {
      root,
      tempo: $(root, "#clock-tempo"),
      lock: $(root, "#clock-lock"),
      beat: $(root, "#clock-beat"),
      beats: [...root.querySelectorAll<HTMLElement>(".beats i")],
      status: $(root, "#source-status"),
      statusDetail: $(root, "#source-detail"),
      nudge: $(root, "#nudge-out"),
      latency: $(root, "#latency"),
      bridgeUrl: $(root, "#bridge-url"),
      bridgeFollow: $(root, "#bridge-follow"),
      devices: $(root, "#bridge-devices"),
      linkUrl: root.querySelector<HTMLInputElement>("#link-url"),
      linkPeersOut: root.querySelector<HTMLElement>("#link-peers"),
      midiInput: $(root, "#midi-input"),
    };

    // Source radios.
    for (const input of root.querySelectorAll<HTMLInputElement>('input[name="clock-source"]')) {
      input.addEventListener("change", () => {
        if (input.checked) this.setSource(input.value as SourceKind);
      });
    }

    // Global controls.
    const nudge = (dir: number) => (e: MouseEvent) => {
      this.setNudge(this.nudgeMs + dir * (e.shiftKey ? 10 : 1));
    };
    $(root, "#nudge-down").addEventListener("click", nudge(-1));
    $(root, "#nudge-up").addEventListener("click", nudge(1));
    $(root, "#nudge-reset").addEventListener("click", () => this.setNudge(0));
    this.latencyMs = Math.max(-250, Math.min(250, loadNumber("latency-ms", 0)));
    this.el.latency.value = String(this.latencyMs);
    host.audio.setLatencyMs(this.latencyMs);
    this.el.latency.addEventListener("change", () => {
      const v = Number(this.el.latency.value);
      this.latencyMs = Number.isFinite(v) ? Math.max(-250, Math.min(250, Math.round(v))) : 0;
      this.el.latency.value = String(this.latencyMs);
      saveString("latency-ms", String(this.latencyMs));
      host.audio.setLatencyMs(this.latencyMs);
    });
    $(root, "#resync").addEventListener("click", () => host.audio.resync());

    // Tap pad: pointerdown carries the most accurate timestamp; keyboard
    // activation (click with detail 0) uses "now".
    const pad = $(root, "#tap-pad");
    pad.addEventListener("pointerdown", (e) => host.tap(e.timeStamp));
    pad.addEventListener("click", (e) => {
      if (e.detail === 0) host.tap(performance.now());
    });

    // Bridge & Ableton Link relay (compiled out of the single-file build).
    this.bridgeUrl = "";
    this.bridgeUrlFromUser = false;
    if (!__P5_SINGLE__ && opts.bridge) {
      const savedUrl = loadString("bridge-url");
      this.bridgeUrlFromUser = savedUrl !== null;
      this.bridgeUrl = savedUrl ?? DEFAULT_BRIDGE_URL;
      this.el.bridgeUrl.value = this.bridgeUrl;
      if (this.el.linkUrl) this.el.linkUrl.value = this.bridgeUrl;

      const applyUrlAndConnect = (rawUrl: string) => {
        const url = rawUrl.trim() || DEFAULT_BRIDGE_URL;
        this.el.bridgeUrl.value = url;
        if (this.el.linkUrl) this.el.linkUrl.value = url;
        this.bridgeUrl = url;
        this.bridgeUrlFromUser = true;
        saveString("bridge-url", url);
        if (this.source === "bridge" || this.source === "link") this.startBridge();
      };

      $(root, "#bridge-connect").addEventListener("click", () =>
        applyUrlAndConnect(this.el.bridgeUrl.value),
      );
      this.el.bridgeUrl.addEventListener("keydown", (e) => {
        if (e.key === "Enter") applyUrlAndConnect(this.el.bridgeUrl.value);
      });
      root.querySelector<HTMLButtonElement>("#link-connect")?.addEventListener("click", () =>
        applyUrlAndConnect(this.el.linkUrl?.value ?? this.el.bridgeUrl.value),
      );
      this.el.linkUrl?.addEventListener("keydown", (e) => {
        if (e.key === "Enter") applyUrlAndConnect(this.el.linkUrl?.value ?? "");
      });

      this.el.bridgeFollow.addEventListener("change", () => {
        const v = this.el.bridgeFollow.value;
        this.followTarget = v === "master" ? "master" : Number(v);
        this.bridge?.follow(this.followTarget);
        this.renderStatus();
      });
    }

    // MIDI.
    this.el.midiInput.addEventListener("change", () => {
      const id = this.el.midiInput.value || null;
      saveString("midi-input", id);
      this.midi?.select(id);
      this.renderStatus();
    });

    this.render(null);
  }

  get following(): boolean {
    return this.source === "link" || this.source === "bridge" || this.source === "midi";
  }

  setSource(source: SourceKind): void {
    if (source === this.source) return;
    const previous = this.source;
    if (previous === "bridge" || previous === "link") {
      this.stopBridge();
      this.stopLocalLinkMesh();
    }
    if (previous === "midi") this.midi?.close();
    this.source = source;
    this.el.root.dataset.source = source;
    const radio = this.el.root.querySelector<HTMLInputElement>(`input[name="clock-source"][value="${source}"]`);
    if (radio) radio.checked = true;
    if (source === "internal" || source === "tap") {
      this.host.audio.setClockMode(0);
    } else if (source === "link") {
      this.host.audio.setClockMode(PRECISION_MODE.exact);
      this.startLocalLinkMesh();
      this.startBridge();
    } else if (source === "bridge") {
      this.host.audio.setClockMode(PRECISION_MODE.fine);
      this.startBridge();
    } else if (source === "midi") {
      this.host.audio.setClockMode(PRECISION_MODE.jittery);
      void this.startMidi();
    }
    this.host.onSourceChange(source, previous);
    this.render(this.host.audio.lastStatus);
  }

  private setNudge(ms: number): void {
    this.nudgeMs = Math.max(-500, Math.min(500, Math.round(ms)));
    this.el.nudge.textContent = `${this.nudgeMs > 0 ? "+" : this.nudgeMs < 0 ? "−" : ""}${Math.abs(this.nudgeMs)} ms`;
    this.host.audio.setNudgeMs(this.nudgeMs);
  }

  // ---- Ableton Link local fallback mesh + LAN Bridge -----------------

  private startLocalLinkMesh(): void {
    this.stopLocalLinkMesh();
    const patBpm = this.host.getPatternBpm?.() ?? 124;
    this.linkBpm = Number.isFinite(patBpm) && patBpm >= 20 && patBpm <= 400 ? patBpm : 124;
    this.linkAnchorEpochMs = Date.now();
    this.linkAnchorBeat = 0;

    if (typeof BroadcastChannel === "function") {
      try {
        const ch = new BroadcastChannel("player5-ableton-link");
        ch.onmessage = (ev: MessageEvent) => {
          const d = ev.data as {
            peer?: string;
            bpm?: number;
            anchorEpochMs?: number;
            anchorBeat?: number;
            bye?: boolean;
          };
          if (!d || typeof d.peer !== "string" || d.peer === this.linkPeerId) return;
          if (d.bye) {
            this.linkPeers.delete(d.peer);
            this.renderStatus();
            return;
          }
          this.linkPeers.set(d.peer, Date.now());
          if (
            typeof d.bpm === "number" &&
            typeof d.anchorEpochMs === "number" &&
            typeof d.anchorBeat === "number"
          ) {
            this.linkBpm = d.bpm;
            this.linkAnchorEpochMs = d.anchorEpochMs;
            this.linkAnchorBeat = d.anchorBeat;
          }
          this.renderStatus();
        };
        this.linkChannel = ch;
      } catch {
        this.linkChannel = null;
      }
    }

    this.linkTimer = setInterval(() => {
      if (this.source !== "link") return;
      const nowEpoch = Date.now();
      for (const [peer, seenAt] of this.linkPeers) {
        if (nowEpoch - seenAt > 3000) this.linkPeers.delete(peer);
      }
      this.linkChannel?.postMessage({
        peer: this.linkPeerId,
        bpm: this.linkBpm,
        anchorEpochMs: this.linkAnchorEpochMs,
        anchorBeat: this.linkAnchorBeat,
      });

      // If the LAN bridge is not delivering a locked Ableton Link timeline yet,
      // drive the engine from the shared Quantum=4 session timeline so Link
      // locks immediately across local browser peers and standalone sessions.
      const bridgeActive =
        this.bridgeState === "open" && this.timeline !== null && this.timeline.locked;
      if (!bridgeActive) {
        const elapsedSec = (nowEpoch - this.linkAnchorEpochMs) / 1000;
        const beat = this.linkAnchorBeat + elapsedSec * (this.linkBpm / 60);
        const barPhase = ((beat % 4) + 4) % 4;
        this.host.audio.observe(performance.now(), 0, barPhase, this.linkBpm);
      }
    }, 50);
  }

  private stopLocalLinkMesh(): void {
    if (this.linkTimer) clearInterval(this.linkTimer);
    this.linkTimer = null;
    if (this.linkChannel) {
      try {
        this.linkChannel.postMessage({ peer: this.linkPeerId, bye: true });
        this.linkChannel.close();
      } catch {
        /* ignore */
      }
    }
    this.linkChannel = null;
    this.linkPeers.clear();
  }

  // ---- bridge --------------------------------------------------------

  private startBridge(): void {
    if (__P5_SINGLE__ || !this.opts.bridge) return;
    this.stopBridge();
    this.timeline = null;
    this.bridgeStatus = "";
    if (!this.bridgeUrlFromUser && !this.discovered) {
      // First use without a saved URL: ask the page's own server whether it
      // is a bridge (GET /bridge.json). Only now, not on every page load:
      // on a plain static host that request is a 404 in the console.
      this.discovered = true;
      this.bridgeState = "discovering";
      this.renderStatus();
      void discoverBridgeUrl().then((url) => {
        if (url && !this.bridgeUrlFromUser) {
          this.bridgeUrl = url;
          this.el.bridgeUrl.value = url;
          if (this.el.linkUrl) this.el.linkUrl.value = url;
        }
        if ((this.source === "bridge" || this.source === "link") && !this.bridge) this.startBridge();
      });
      return;
    }
    const client = new BridgeClient(this.bridgeUrl, {
      onConnection: (state, detail) => {
        this.bridgeState = state;
        if (state === "closed") this.timeline = null;
        // The bridge refuses pages from origins it does not trust and says
        // how to allow one; fill in ours.
        if (detail && state === "closed")
          this.bridgeStatus = `closed (${detail.replace("<origin>", location.origin)})`;
        this.renderStatus();
      },
      onHello: () => {
        this.bridgeStatus = "";
        this.renderStatus();
      },
      onTimeline: (t) => {
        this.timeline = t;
        this.host.audio.setClockMode(PRECISION_MODE[t.precision]);
        this.renderStatus();
      },
      onDeviceChange: () => this.host.audio.resync(),
      onDevices: (devices) => this.renderDevices(devices),
      onStatus: (level, message) => {
        this.bridgeStatus = `${level}: ${message}`;
        this.renderStatus();
      },
      onObservation: (ms, kind, phase, bpm) => {
        this.host.audio.observe(ms, kind, phase, bpm);
      },
    });
    this.bridge = client;
    // Tell the bridge whether we want Ableton Link ("link") or Pioneer Pro DJ Link ("prolink")
    if (this.source === "link") {
      client.setSource("link");
    } else if (this.source === "bridge") {
      client.setSource("prolink");
    }
    if (this.followTarget !== "master") client.follow(this.followTarget);
    client.connect();
  }

  private stopBridge(): void {
    this.bridge?.close();
    this.bridge = null;
    this.timeline = null;
    this.bridgeState = "closed";
  }

  private renderDevices(devices: BridgeDevice[]): void {
    const list = this.el.devices;
    list.replaceChildren(
      ...devices.map((d) => {
        const li = document.createElement("li");
        li.dataset.device = String(d.number);
        const flags = [
          d.kind,
          typeof d.bpm === "number" ? `${d.bpm.toFixed(2)} BPM` : null,
          d.playing === true ? "playing" : d.playing === false ? "paused" : null,
          d.master ? "master" : null,
          d.on_air ? "on air" : null,
        ].filter(Boolean);
        const num = document.createElement("span");
        num.className = "dev-num";
        num.textContent = String(d.number);
        const name = document.createElement("span");
        name.className = "dev-name";
        name.textContent = d.name || "device";
        const info = document.createElement("span");
        info.className = "dev-info";
        info.textContent = flags.join(" · ");
        li.append(num, name, info);
        if (d.master) li.classList.add("master");
        return li;
      }),
    );
    // Follow targets: the master, or any player.
    const select = this.el.bridgeFollow;
    const current = String(this.followTarget);
    const options = [new Option("Tempo master", "master")];
    for (const d of devices) {
      if (d.kind === "player" || d.kind === "all-in-one") {
        options.push(new Option(`${d.number} · ${d.name || "player"}`, String(d.number)));
      }
    }
    if (!options.some((o) => o.value === current)) options.push(new Option(`${current} · not seen`, current));
    select.replaceChildren(...options);
    select.value = current;
  }

  // ---- MIDI ----------------------------------------------------------

  private async startMidi(): Promise<void> {
    if (!this.opts.midi) return;
    this.midiError = "";
    if (!this.midi) {
      this.midi = new MidiClockInput({
        onClock: (code, ms) => {
          if (this.source !== "midi") return;
          this.host.audio.midi(code, ms);
        },
        onInputs: (ports) => this.renderMidiPorts(ports),
      });
    }
    try {
      const ports = await this.midi.open();
      if (this.source !== "midi") return;
      const saved = loadString("midi-input");
      const pick = ports.find((p) => p.id === saved) ?? ports[0];
      this.midi.select(pick?.id ?? null);
      this.renderMidiPorts(ports);
    } catch (err) {
      this.midiError = err instanceof Error ? err.message : String(err);
    }
    this.renderStatus();
  }

  private renderMidiPorts(ports: MidiPort[]): void {
    this.midiPorts = ports;
    const select = this.el.midiInput;
    const options = [new Option(ports.length ? "— none —" : "no MIDI inputs", "")];
    for (const p of ports) options.push(new Option(p.name, p.id));
    select.replaceChildren(...options);
    select.value = this.midi?.selectedId ?? "";
    this.renderStatus();
  }

  // ---- readouts ------------------------------------------------------

  onAudioState(state: AudioState): void {
    this.audioState = state;
    this.render(this.host.audio.lastStatus);
  }

  onStatus(status: EngineStatus): void {
    this.render(status);
  }

  private render(status: EngineStatus | null): void {
    const running = this.audioState === "running" && status !== null;
    const following = this.following;
    const lock = !running ? "off" : !following ? "internal" : status.locked ? "locked" : "searching";
    this.el.root.dataset.lock = lock;
    this.el.tempo.textContent = running ? status.tempo.toFixed(2) : "—";
    this.el.root.dataset.tempo = running ? status.tempo.toFixed(3) : "";
    const beat = running ? (this.host.audio.beatHeardAt(performance.now()) ?? status.beat) : NaN;
    this.el.root.dataset.beat = running ? beat.toFixed(4) : "";
    this.el.root.dataset.beatAt = running ? performance.now().toFixed(2) : "";
    this.el.lock.textContent =
      lock === "off" ? "audio off" : lock === "internal" ? "internal" : lock === "locked" ? "locked" : "searching";
    let pos = -1;
    if (running && Number.isFinite(beat)) pos = Math.floor((((beat % 4) + 4) % 4) + 1e-9);
    this.el.beat.textContent = pos >= 0 ? String(pos + 1) : "–";
    this.el.beats.forEach((b, i) => b.classList.toggle("on", i === pos));
    this.renderStatus();
  }

  private renderStatus(): void {
    let text = "";
    let detail = "";
    const running = this.audioState === "running";
    switch (this.source) {
      case "internal":
        text = "Free-running at the pattern tempo.";
        break;
      case "tap":
        text = "Tap the beat: sets the tempo and pulls the grid onto your taps.";
        break;
      case "link": {
        const t = this.timeline;
        const rtt = this.bridge?.bestRttMs;
        if (this.bridgeState === "open" && t) {
          text = `Ableton Link — Enabled · ${t.locked ? "locked" : "searching"} · ${t.bpm.toFixed(2)} BPM · quantum 4 beats`;
          if (this.bridgeStatus) text += ` (${this.bridgeStatus.replace(/^info:\s*/i, "")})`;
          if (typeof rtt === "number") detail = `rtt ${rtt.toFixed(1)} ms`;
          if (this.el.linkPeersOut) {
            const m = /(\d+)\s+peer/i.exec(this.bridgeStatus);
            this.el.linkPeersOut.textContent = m ? `${m[1]} LAN peer(s) via bridge` : "LAN session active via bridge";
          }
        } else {
          const nPeers = this.linkPeers.size;
          text = `Ableton Link — Enabled · ${this.linkBpm.toFixed(2)} BPM · quantum 4 beats · ${
            nPeers === 0 ? "waiting for Ableton Live / Link peers" : `${nPeers} local peer(s)`
          }`;
          if (this.el.linkPeersOut) {
            this.el.linkPeersOut.textContent =
              nPeers > 0 ? `${nPeers} browser peer(s)` : "0 peers (connect bridge for LAN UDP 20808)";
          }
        }
        this.el.root.dataset.bridge = this.bridgeState;
        break;
      }
      case "bridge": {
        const t = this.timeline;
        if (this.bridgeState === "discovering") text = "Looking for a bridge…";
        else if (this.bridgeState === "connecting") text = `Connecting to ${this.bridgeUrl}…`;
        else if (this.bridgeState === "closed") text = `Bridge offline (${this.bridgeUrl}); retrying.`;
        else if (!t) text = "Connected; waiting for a timeline.";
        else {
          const rtt = this.bridge?.bestRttMs;
          text =
            `${t.source} · ${t.locked ? "locked" : "searching"} · ${t.bpm.toFixed(2)} BPM · ${t.precision}` +
            (t.device !== null ? ` · device ${t.device}` : "") +
            (this.bridge && !this.bridge.onTarget ? ` · switching to device ${String(this.bridge.target)}` : "");
          if (typeof rtt === "number") detail = `rtt ${rtt.toFixed(1)} ms`;
        }
        if (this.bridgeStatus) text += ` — ${this.bridgeStatus}`;
        this.el.root.dataset.bridge = this.bridgeState;
        break;
      }
      case "midi": {
        if (this.midiError) text = `MIDI unavailable: ${this.midiError}`;
        else if (!this.midiPorts.length) text = "No MIDI inputs found.";
        else {
          const port = this.midiPorts.find((p) => p.id === this.midi?.selectedId);
          text = port ? `Listening to ${port.name}` : "Pick a MIDI input.";
          if (port) detail = `${this.midi?.clocks ?? 0} clocks`;
        }
        break;
      }
    }
    if (this.following && !running) {
      text += `${/[.!?]$/.test(text) ? " " : ". "}Press Play to start the engine; it joins the source in phase.`;
    }
    if (this.el.status.textContent !== text) this.el.status.textContent = text;
    const shown = detail ? ` · ${detail}` : "";
    if (this.el.statusDetail.textContent !== shown) this.el.statusDetail.textContent = shown;
  }
}
