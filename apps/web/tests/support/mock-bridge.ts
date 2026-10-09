// A minimal bridge (docs/protocols/bridge-websocket.md, protocol 1) for
// tests: RFC 6455 handshake and framing on node:http + node:crypto, no
// WebSocket dependency. Speaks hello, pong, timeline (10 Hz), devices and
// status; records what the client sends.

import { createHash } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import type { AddressInfo, Socket } from "node:net";

const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

export interface MockBridgeOptions {
  bpm?: number;
  precision?: "exact" | "fine" | "coarse" | "jittery";
  /** Port to listen on (default: any free port). */
  port?: number;
}

export interface MockBridge {
  url: string;
  received: Record<string, unknown>[];
  /** The timeline's beat right now (server clock). */
  beatNow(): number;
  close(): Promise<void>;
}

function frame(text: string): Buffer {
  const payload = Buffer.from(text, "utf8");
  const n = payload.length;
  const head = n < 126 ? Buffer.alloc(2) : n < 65536 ? Buffer.alloc(4) : Buffer.alloc(10);
  head[0] = 0x81; // FIN + text
  if (n < 126) head[1] = n;
  else if (n < 65536) {
    head[1] = 126;
    head.writeUInt16BE(n, 2);
  } else {
    head[1] = 127;
    head.writeBigUInt64BE(BigInt(n), 2);
  }
  return Buffer.concat([head, payload]);
}

/** Parses complete client frames (always masked) off the front of `buf`. */
function parseFrames(buf: Buffer): { frames: { opcode: number; payload: Buffer }[]; rest: Buffer } {
  const frames: { opcode: number; payload: Buffer }[] = [];
  let off = 0;
  for (;;) {
    if (buf.length - off < 2) break;
    const opcode = buf[off]! & 0x0f;
    const masked = (buf[off + 1]! & 0x80) !== 0;
    let len = buf[off + 1]! & 0x7f;
    let p = off + 2;
    if (len === 126) {
      if (buf.length - p < 2) break;
      len = buf.readUInt16BE(p);
      p += 2;
    } else if (len === 127) {
      if (buf.length - p < 8) break;
      len = Number(buf.readBigUInt64BE(p));
      p += 8;
    }
    const maskLen = masked ? 4 : 0;
    if (buf.length - p < maskLen + len) break;
    const mask = masked ? buf.subarray(p, p + 4) : null;
    p += maskLen;
    const payload = Buffer.from(buf.subarray(p, p + len));
    if (mask) for (let i = 0; i < payload.length; i++) payload[i]! ^= mask[i & 3]!;
    frames.push({ opcode, payload });
    off = p + len;
  }
  return { frames, rest: buf.subarray(off) };
}

export async function startMockBridge(opts: MockBridgeOptions = {}): Promise<MockBridge> {
  const bpm = opts.bpm ?? 128;
  const precision = opts.precision ?? "fine";
  // Server clock: monotonic microseconds with an arbitrary epoch, far from
  // the client's performance.now() so a wrong offset would show.
  const epoch = process.hrtime.bigint();
  const serverUs = () => Number((process.hrtime.bigint() - epoch) / 1000n) + 987_654_321;
  const anchorUs = serverUs();
  const anchorBeat = 1033.25;
  const received: Record<string, unknown>[] = [];
  const sockets = new Set<Socket>();
  const timers = new Set<ReturnType<typeof setInterval>>();

  const devices = [
    { number: 2, name: "Mock Deck", address: "127.0.0.1", kind: "player", bpm, playing: true, master: true, on_air: true },
    { number: 3, name: "Other Deck", address: "127.0.0.2", kind: "player", bpm: 122, playing: false, master: false, on_air: false },
    { number: 33, name: "Mock Mixer", address: "127.0.0.3", kind: "mixer", bpm: null, playing: null, master: null, on_air: null },
  ];

  const server = createServer((req, res) => {
    if (req.url === "/bridge.json") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ protocol: 1, ws: "/ws" }));
      return;
    }
    res.writeHead(404);
    res.end();
  });

  server.on("upgrade", (req: IncomingMessage, socket: Socket) => {
    const key = req.headers["sec-websocket-key"];
    if (req.url !== "/ws" || typeof key !== "string") {
      socket.end("HTTP/1.1 400 Bad Request\r\n\r\n");
      return;
    }
    const accept = createHash("sha1").update(key + GUID).digest("base64");
    socket.write(
      "HTTP/1.1 101 Switching Protocols\r\n" +
        "Upgrade: websocket\r\n" +
        "Connection: Upgrade\r\n" +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    sockets.add(socket);
    const send = (msg: object) => {
      if (!socket.destroyed) socket.write(frame(JSON.stringify(msg)));
    };
    send({ type: "hello", protocol: 1, app: "player5-bridge", version: "0.0.0-mock", server_us: serverUs(), source: "sim" });
    send({ type: "devices", devices });
    send({ type: "status", level: "info", message: "mock bridge ready" });
    send({ type: "future-message", ignored: true }); // clients must ignore unknown types
    const timeline = () =>
      send({
        type: "timeline",
        source: "sim",
        locked: true,
        bpm,
        anchor_us: anchorUs,
        anchor_beat: anchorBeat,
        bar_aligned: true,
        precision,
        device: 2,
      });
    timeline();
    const timer = setInterval(timeline, 100);
    timers.add(timer);

    let pending: Buffer = Buffer.alloc(0);
    socket.on("data", (chunk: Buffer) => {
      const { frames, rest } = parseFrames(Buffer.concat([pending, chunk]));
      pending = rest;
      for (const f of frames) {
        if (f.opcode === 0x8) {
          socket.end(Buffer.from([0x88, 0x00]));
          return;
        }
        if (f.opcode === 0x9) {
          socket.write(Buffer.concat([Buffer.from([0x8a, f.payload.length]), f.payload]));
          continue;
        }
        if (f.opcode !== 0x1) continue;
        let msg: Record<string, unknown>;
        try {
          msg = JSON.parse(f.payload.toString("utf8")) as Record<string, unknown>;
        } catch {
          continue;
        }
        received.push(msg);
        if (msg.type === "ping") {
          send({ type: "pong", id: msg.id, client_ms: msg.client_ms, server_us: serverUs() });
        }
      }
    });
    const cleanup = () => {
      clearInterval(timer);
      timers.delete(timer);
      sockets.delete(socket);
    };
    socket.on("close", cleanup);
    socket.on("error", cleanup);
  });

  await new Promise<void>((resolve) => server.listen(opts.port ?? 0, "127.0.0.1", resolve));
  const { port } = server.address() as AddressInfo;
  return {
    url: `ws://127.0.0.1:${port}/ws`,
    received,
    beatNow: () => anchorBeat + ((serverUs() - anchorUs) / 1e6) * (bpm / 60),
    close: async () => {
      for (const t of timers) clearInterval(t);
      for (const s of sockets) s.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}
