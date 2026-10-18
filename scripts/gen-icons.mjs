// Draws Ciel into the PNG/ICO set Tauri needs. No dependencies: the icons are
// rasterised here and encoded with node:zlib, so the app icon stays "drawn in
// code" like the character itself.
//
//   node scripts/gen-icons.mjs

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { EXTENT, insideCloud } from "../src/cloud/shape.ts";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");

// ── Ciel ─────────────────────────────────────────────────────────────────────

const BASE_TOP = [255, 255, 255]; // #FFFFFF
const BASE_BOTTOM = [202, 221, 242]; // #CADDF2
const INK = [30, 41, 72]; // #1E2948
const RIM = [30, 41, 72]; // the same navy, so the tray icon reads on light themes
const GLINT = [255, 255, 255];

const SS = 4; // supersampling factor

/** Features a little bigger than in the app, so they survive at 16 px. */
const FEATURE = 1.3;

function insideEllipse(x, y, w, h) {
  return (x / (w / 2)) ** 2 + (y / (h / 2)) ** 2 <= 1;
}

/** On the smile's arc: a ring of radius r and width lw, between two angles (y down). */
function onSmile(x, y, r, lw, a0, a1) {
  const d = Math.hypot(x, y);
  if (Math.abs(d - r) > lw / 2) {
    // The round caps at both ends.
    for (const a of [a0, a1]) {
      if (Math.hypot(x - Math.cos(a) * r, y - Math.sin(a) * r) <= lw / 2) return true;
    }
    return false;
  }
  const a = Math.atan2(y, x);
  return a >= a0 && a <= a1;
}

function renderCloud(size) {
  const px = new Uint8Array(size * size * 4);
  const R = size * 0.42;
  const cx = size / 2;
  const cy = size / 2 - EXTENT.dy * R;
  const rim = 0.06; // in units of R

  // Eyes and mouth: the engine's geometry (yaw ±0.4, pitch −0.08, rx 1.14 R, ry 0.88 R).
  const ex = Math.sin(0.4) * Math.cos(0.08) * 1.14;
  const ey = Math.sin(0.08) * 0.88;
  const ew = 0.2 * Math.cos(0.4) * FEATURE;
  const eh = 0.23 * FEATURE;
  const smileR = 0.09 * FEATURE;
  const smileY = ey + 0.25 - 0.05 * FEATURE;
  const smileW = 0.05 * FEATURE;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let rimHits = 0;
      let bodyHits = 0;
      let inkHits = 0;
      let glintHits = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const ux = (x + (sx + 0.5) / SS - cx) / R;
          const uy = (y + (sy + 0.5) / SS - cy) / R;
          if (!insideCloud(ux / (1 + rim), uy / (1 + rim))) continue;
          rimHits++;
          if (!insideCloud(ux, uy)) continue;
          bodyHits++;
          let eye = false;
          for (const sd of [-1, 1]) {
            const lx = ux - sd * ex;
            const ly = uy - ey;
            if (insideEllipse(lx, ly, ew, eh)) {
              eye = true;
              if (Math.hypot(lx + ew * 0.16, ly + eh * 0.2) <= ew * 0.2) glintHits++;
              else inkHits++;
            }
          }
          if (!eye && onSmile(ux, uy - smileY, smileR, smileW, Math.PI * 0.22, Math.PI * 0.78)) inkHits++;
        }
      }
      if (rimHits === 0) continue;

      const total = SS * SS;
      const rimA = rimHits / total;
      const bodyA = bodyHits / total;
      const inkA = inkHits / total;
      const glintA = glintHits / total;

      // Body gradient: white on top, the sky's blue underneath.
      const t = Math.min(1, Math.max(0, (y - cy) / (1.7 * R) + 0.5));
      const body = [0, 1, 2].map((i) => BASE_TOP[i] + (BASE_BOTTOM[i] - BASE_TOP[i]) * t);

      // rim under body, body over rim, ink and glint over body
      let col = RIM.slice();
      if (bodyA > 0) col = col.map((c, i) => c * (1 - bodyA / rimA) + body[i] * (bodyA / rimA));
      if (inkA > 0) col = col.map((c, i) => c * (1 - inkA) + INK[i] * inkA);
      if (glintA > 0) col = col.map((c, i) => c * (1 - glintA) + GLINT[i] * glintA);

      const o = (y * size + x) * 4;
      px[o] = Math.round(col[0]);
      px[o + 1] = Math.round(col[1]);
      px[o + 2] = Math.round(col[2]);
      px[o + 3] = Math.round(Math.min(1, rimA) * 255);
    }
  }
  return px;
}

// ── PNG ───────────────────────────────────────────────────────────────────────

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
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePNG(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // filter: none
    Buffer.from(rgba.buffer, y * size * 4, size * 4).copy(raw, y * (size * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// ── ICO (PNG-in-ICO, Vista and later) ─────────────────────────────────────────

function encodeICO(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = header.length + dir.length;
  entries.forEach((e, i) => {
    const o = i * 16;
    dir[o] = e.size >= 256 ? 0 : e.size;
    dir[o + 1] = e.size >= 256 ? 0 : e.size;
    dir[o + 2] = 0;
    dir[o + 3] = 0;
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(e.png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += e.png.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.png)]);
}

// ── Go ────────────────────────────────────────────────────────────────────────

mkdirSync(OUT, { recursive: true });

const png = (size) => encodePNG(size, renderCloud(size));

const files = {
  "32x32.png": png(32),
  "128x128.png": png(128),
  "128x128@2x.png": png(256),
  "icon.png": png(512),
};
for (const [name, data] of Object.entries(files)) {
  writeFileSync(join(OUT, name), data);
  console.log(`${name} — ${data.length} bytes`);
}

const ico = encodeICO([16, 24, 32, 48, 64, 128, 256].map((size) => ({ size, png: png(size) })));
writeFileSync(join(OUT, "icon.ico"), ico);
console.log(`icon.ico — ${ico.length} bytes`);
