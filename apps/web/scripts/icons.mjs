// App icons for the PWA manifest, drawn procedurally and encoded as PNG
// with Node's zlib: no image dependencies. vite.player5.ts emits them at
// build time; `node scripts/icons.mjs <dir>` writes them for inspection.
//
// The logo: four orange level bars rising left to right on the panel
// colour, with a cyan playhead tick under the third bar.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

const BG = [0x0f, 0x11, 0x15];
const ORANGE = [0xff, 0x7a, 0x1a];
const CYAN = [0x6c, 0xf0, 0xff];

// ---- PNG encoding ----

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, "ascii");
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}

/** Encodes RGBA pixels (Uint8Array, width*height*4) as a PNG. */
export function encodePng(width, height, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter: none
    Buffer.from(rgba.buffer, rgba.byteOffset + y * width * 4, width * 4).copy(raw, y * (width * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// ---- drawing ----

/** Signed distance to a rounded rectangle (center cx, cy; half sizes hw, hh; radius r). */
function roundRect(x, y, cx, cy, hw, hh, r) {
  const qx = Math.abs(x - cx) - hw + r;
  const qy = Math.abs(y - cy) - hh + r;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
}

/**
 * Renders the logo at `size` px. `maskable`: full-bleed background with the
 * mark inside the central safe zone (radius 40 %), as maskable icons need.
 */
export function renderIcon(size, { maskable = false } = {}) {
  const px = new Uint8Array(size * size * 4);
  const SS = 4; // supersampling per axis
  // Shapes in a unit square.
  const scale = maskable ? 0.62 : 0.8;
  const off = (1 - scale) / 2;
  const bars = [0.3, 0.48, 0.66, 0.84].map((h, i) => ({
    cx: off + scale * (0.2 + i * 0.2),
    cy: off + scale * (0.88 - h / 2),
    hw: scale * 0.075,
    hh: (scale * h) / 2,
  }));
  const tick = { cx: bars[2].cx, cy: off + scale * 0.95, hw: scale * 0.075, hh: scale * 0.018 };
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let a = 0;
      let r = 0;
      let g = 0;
      let b = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const u = (x + (sx + 0.5) / SS) / size;
          const v = (y + (sy + 0.5) / SS) / size;
          const inBg = maskable || roundRect(u, v, 0.5, 0.5, 0.5, 0.5, 0.18) <= 0;
          if (!inBg) continue;
          let col = BG;
          for (const bar of bars) {
            if (roundRect(u, v, bar.cx, bar.cy, bar.hw, bar.hh, bar.hw * 0.45) <= 0) col = ORANGE;
          }
          if (roundRect(u, v, tick.cx, tick.cy, tick.hw, tick.hh, tick.hh) <= 0) col = CYAN;
          a += 1;
          r += col[0];
          g += col[1];
          b += col[2];
        }
      }
      const i = (y * size + x) * 4;
      if (a > 0) {
        px[i] = Math.round(r / a);
        px[i + 1] = Math.round(g / a);
        px[i + 2] = Math.round(b / a);
      }
      px[i + 3] = Math.round((a / (SS * SS)) * 255);
    }
  }
  return encodePng(size, size, px);
}

/** The icon set the manifest lists: published path → PNG bytes. */
export function iconSet() {
  return {
    "icons/icon-192.png": renderIcon(192),
    "icons/icon-512.png": renderIcon(512),
    "icons/maskable-512.png": renderIcon(512, { maskable: true }),
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const dir = process.argv[2] ?? ".";
  for (const [name, png] of Object.entries(iconSet())) {
    const path = join(dir, name);
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, png);
    console.log(`${path} (${png.length} bytes)`);
  }
}
