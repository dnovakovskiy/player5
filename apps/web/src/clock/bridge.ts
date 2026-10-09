// BridgeClock: the browser client of docs/protocols/bridge-websocket.md
// (protocol version 1). apps/bridge joins the booth LAN (Pro DJ Link,
// Ableton Link, ...) and serves its timeline over a WebSocket; this class
// estimates the server clock offset, turns timelines into observations on
// the performance clock, and keeps the connection alive.

export type Precision = "exact" | "fine" | "coarse" | "jittery";

/** Clock-mode codes of core/ffi for each precision. */
export const PRECISION_MODE: Record<Precision, number> = { exact: 1, fine: 2, coarse: 3, jittery: 4 };

export interface BridgeDevice {
  number: number;
  name: string;
  address?: string;
  kind: string;
  bpm: number | null;
  playing: boolean | null;
  master: boolean | null;
  on_air: boolean | null;
}

export interface BridgeTimeline {
  source: string;
  locked: boolean;
  bpm: number;
  anchor_us: number;
  anchor_beat: number;
  bar_aligned: boolean;
  precision: Precision;
  device: number | null;
}

export type BridgeConnection = "connecting" | "open" | "closed";

export interface BridgeEvents {
  onConnection(state: BridgeConnection, detail?: string): void;
  onHello?(hello: { app?: string; version?: string; source?: string }): void;
  onTimeline(timeline: BridgeTimeline): void;
  onDevices(devices: BridgeDevice[]): void;
  onStatus(level: "info" | "warn" | "error", message: string): void;
  /** ~20 Hz while locked: at performance time `ms` the source was at `phase`. */
  onObservation(ms: number, kind: number, phase: number, bpm: number, precision: Precision): void;
}

export const DEFAULT_BRIDGE_URL = "ws://localhost:17505/ws";

const PING_BURST = 8;
const PING_BURST_SPACING_MS = 40;
const PING_INTERVAL_MS = 2000;
const OFFSET_WINDOW = 16;
const OBSERVE_INTERVAL_MS = 50;
const BACKOFF_MIN_MS = 500;
const BACKOFF_MAX_MS = 10_000;

/**
 * Looks for a bridge serving this page: GET /bridge.json on the same
 * origin. Returns its WebSocket URL, or null.
 */
export async function discoverBridgeUrl(): Promise<string | null> {
  if (__P5_SINGLE__) return null;
  try {
    if (!/^https?:$/.test(location.protocol)) return null;
    const res = await fetch(new URL("/bridge.json", location.origin).href, { cache: "no-store" });
    if (!res.ok) return null;
    const info = (await res.json()) as { protocol?: unknown; ws?: unknown };
    if (info.protocol !== 1 || typeof info.ws !== "string") return null;
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    return `${scheme}//${location.host}${info.ws.startsWith("/") ? "" : "/"}${info.ws}`;
  } catch {
    return null;
  }
}

/** Beat at server time `serverUs` per the protocol's timeline formula. */
export function beatAt(t: BridgeTimeline, serverUs: number): number {
  return t.anchor_beat + ((serverUs - t.anchor_us) / 1e6) * (t.bpm / 60);
}

function isPrecision(p: unknown): p is Precision {
  return p === "exact" || p === "fine" || p === "coarse" || p === "jittery";
}

export class BridgeClient {
  private ws: WebSocket | null = null;
  private closedByUser = false;
  private attempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private burstTimers: ReturnType<typeof setTimeout>[] = [];
  private observeTimer: ReturnType<typeof setInterval> | null = null;
  private nextPingId = 1;
  private samples: { rtt: number; offsetUs: number }[] = [];
  /** server_us − client_us from the lowest-RTT recent exchange. */
  offsetUs: number | null = null;
  bestRttMs: number | null = null;
  timeline: BridgeTimeline | null = null;
  devices: BridgeDevice[] = [];
  connection: BridgeConnection = "closed";

  constructor(
    readonly url: string,
    private readonly events: BridgeEvents,
  ) {}

  connect(): void {
    this.closedByUser = false;
    this.open();
  }

  close(): void {
    this.closedByUser = true;
    this.clearTimers();
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
    const ws = this.ws;
    this.ws = null;
    if (ws) {
      ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
      try {
        ws.close();
      } catch {
        /* already closed */
      }
    }
    this.setConnection("closed");
  }

  /** Selects the followed device: "master" or a device number. */
  follow(target: "master" | number): void {
    this.sendJson({ type: "follow", target });
  }

  private open(): void {
    this.setConnection("connecting");
    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url);
    } catch (err) {
      this.events.onStatus("error", `bad bridge URL: ${err instanceof Error ? err.message : String(err)}`);
      this.scheduleReconnect();
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      this.samples = [];
      this.offsetUs = null;
      this.bestRttMs = null;
      this.setConnection("open");
      // Burst first for a quick offset estimate, then keep refreshing it.
      for (let i = 0; i < PING_BURST; i++) {
        this.burstTimers.push(setTimeout(() => this.ping(), i * PING_BURST_SPACING_MS));
      }
      this.pingTimer = setInterval(() => this.ping(), PING_INTERVAL_MS);
      this.observeTimer = setInterval(() => this.emitObservation(), OBSERVE_INTERVAL_MS);
    };
    ws.onmessage = (e: MessageEvent) => {
      if (typeof e.data === "string") this.onText(e.data);
    };
    ws.onerror = () => {
      /* onclose follows with the details */
    };
    ws.onclose = (e: CloseEvent) => {
      this.ws = null;
      this.clearTimers();
      this.timeline = null;
      this.setConnection("closed", e.reason || (e.code ? `code ${e.code}` : undefined));
      if (!this.closedByUser) this.scheduleReconnect();
    };
  }

  private scheduleReconnect(): void {
    if (this.closedByUser || this.reconnectTimer) return;
    const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** this.attempt);
    const delay = base * (0.75 + Math.random() * 0.5);
    this.attempt++;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.closedByUser) this.open();
    }, delay);
  }

  private clearTimers(): void {
    if (this.pingTimer) clearInterval(this.pingTimer);
    if (this.observeTimer) clearInterval(this.observeTimer);
    for (const t of this.burstTimers) clearTimeout(t);
    this.pingTimer = null;
    this.observeTimer = null;
    this.burstTimers = [];
  }

  private sendJson(msg: object): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg));
  }

  private ping(): void {
    this.sendJson({ type: "ping", id: this.nextPingId++, client_ms: performance.now() });
  }

  private onText(text: string): void {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(text) as Record<string, unknown>;
    } catch {
      return;
    }
    if (!msg || typeof msg !== "object") return;
    switch (msg.type) {
      case "hello":
        this.attempt = 0;
        if (msg.protocol !== 1) {
          this.events.onStatus("warn", `bridge speaks protocol ${String(msg.protocol)}, expected 1`);
        }
        this.events.onHello?.(msg as { app?: string; version?: string; source?: string });
        break;
      case "pong":
        this.onPong(msg);
        break;
      case "timeline":
        this.onTimeline(msg);
        break;
      case "devices":
        if (Array.isArray(msg.devices)) {
          this.devices = (msg.devices as BridgeDevice[]).filter(
            (d) => d && typeof d === "object" && typeof d.number === "number",
          );
          this.events.onDevices(this.devices);
        }
        break;
      case "status": {
        const level = msg.level === "warn" || msg.level === "error" ? msg.level : "info";
        if (typeof msg.message === "string") this.events.onStatus(level, msg.message);
        break;
      }
      default:
        break; // unknown types are ignored (forward compatibility)
    }
  }

  private onPong(msg: Record<string, unknown>): void {
    const clientMs = Number(msg.client_ms);
    const serverUs = Number(msg.server_us);
    if (!Number.isFinite(clientMs) || !Number.isFinite(serverUs)) return;
    const now = performance.now();
    const rtt = now - clientMs;
    if (rtt < 0) return;
    const offsetUs = serverUs - (clientMs + rtt / 2) * 1000;
    this.samples.push({ rtt, offsetUs });
    if (this.samples.length > OFFSET_WINDOW) this.samples.shift();
    let best = this.samples[0]!;
    for (const s of this.samples) if (s.rtt < best.rtt) best = s;
    this.offsetUs = best.offsetUs;
    this.bestRttMs = best.rtt;
    this.emitObservation();
  }

  private onTimeline(msg: Record<string, unknown>): void {
    const bpm = Number(msg.bpm);
    const anchorUs = Number(msg.anchor_us);
    const anchorBeat = Number(msg.anchor_beat);
    if (![bpm, anchorUs, anchorBeat].every(Number.isFinite)) return;
    this.timeline = {
      source: typeof msg.source === "string" ? msg.source : "none",
      locked: msg.locked === true,
      bpm,
      anchor_us: anchorUs,
      anchor_beat: anchorBeat,
      bar_aligned: msg.bar_aligned === true,
      precision: isPrecision(msg.precision) ? msg.precision : "fine",
      device: typeof msg.device === "number" ? msg.device : null,
    };
    this.events.onTimeline(this.timeline);
    this.emitObservation();
  }

  /** Beat now, mapped onto the performance clock, while the source is locked. */
  private emitObservation(): void {
    const t = this.timeline;
    if (!t || !t.locked || this.offsetUs === null || !(t.bpm > 0)) return;
    const ms = performance.now();
    const beat = beatAt(t, ms * 1000 + this.offsetUs);
    const kind = t.bar_aligned ? 0 : 1;
    const phase = t.bar_aligned ? ((beat % 4) + 4) % 4 : ((beat % 1) + 1) % 1;
    this.events.onObservation(ms, kind, phase, t.bpm, t.precision);
  }

  private setConnection(state: BridgeConnection, detail?: string): void {
    this.connection = state;
    this.events.onConnection(state, detail);
  }
}
