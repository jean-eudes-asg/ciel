// Ciel's 29 sounds, synthesized from code. No samples and no dependencies: each
// sound is a small recipe of low, round tones (a tap on wood, a deep bell),
// smooth envelopes and a breath of filtered noise, written as 16-bit mono
// 44.1 kHz WAV into sounds/.
//
// The output is deterministic (the noise comes from a seeded PRNG), so running
// it again rewrites byte-identical files unless a recipe changed.
//
//   npm run sounds        (= node scripts/gen-sounds.mjs)
//
// The names must be exactly SOUND_NAMES in src/core/sound.ts; the script checks.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "sounds");

// ── Synth toolkit ────────────────────────────────────────────────────────────

const SR = 44100;
const TAU = 2 * Math.PI;
const PEAK = 10 ** (-3 / 20); // every file peaks at -3 dBFS

const frames = (sec) => Math.round(sec * SR);
const clamp01 = (x) => Math.max(0, Math.min(1, x));
/** Smoothstep: eases 0 → 1 with no corners at either end. */
const smooth = (x) => {
  const k = clamp01(x);
  return k * k * (3 - 2 * k);
};

/** Hz of a note name: "A4" = 440, "C#6", "Bb3". */
function note(name) {
  const m = /^([A-G])([#b]?)(\d)$/.exec(name);
  if (!m) throw new Error(`not a note: ${name}`);
  const step = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 }[m[1]];
  const midi = 12 * (Number(m[3]) + 1) + step + (m[2] === "#" ? 1 : m[2] === "b" ? -1 : 0);
  return 440 * 2 ** ((midi - 69) / 12);
}

/** A pitch through [time s, Hz] points, easing evenly in pitch between each pair. */
function path(points) {
  return (t) => {
    if (t <= points[0][0]) return points[0][1];
    for (let i = 1; i < points.length; i++) {
      const [t1, f1] = points[i];
      if (t <= t1) {
        const [t0, f0] = points[i - 1];
        return f0 * (f1 / f0) ** smooth((t - t0) / (t1 - t0));
      }
    }
    return points[points.length - 1][1];
  };
}

/** A pitch sliding from f0 to f1 Hz over `sec`, then holding f1. */
const glide = (f0, f1, sec) => path([[0, f0], [sec, f1]]);

/** f (Hz or t => Hz) times k. */
const times = (f, k) => (typeof f === "function" ? (t) => f(t) * k : f * k);

/** Wave shapes over a phase in radians; all of them soft and alias-free. */
const WAVES = {
  sine: (p) => Math.sin(p),
  // a triangle's first three partials: hollow and gentle, nothing above the 5th
  tri: (p) => 0.81 * (Math.sin(p) - Math.sin(3 * p) / 9 + Math.sin(5 * p) / 25),
  // a sine with a quiet octave and twelfth: rounder, a little like a flute
  soft: (p) => 0.8 * (Math.sin(p) + 0.22 * Math.sin(2 * p) + 0.07 * Math.sin(3 * p)),
};

/**
 * Envelope (t, sec) => 0..1: a raised-cosine attack of `a` s, an exponential
 * fall (time constant `d` s) toward the sustain level `s`, and a raised-cosine
 * release over the note's last `r` s. s = 0 gives a bell or a pluck.
 */
function adsr({ a = 0.01, d = 0.1, s = 0, r = 0.03 } = {}) {
  return (t, sec) => {
    let v = t < a ? 0.5 - 0.5 * Math.cos((Math.PI * t) / a) : s + (1 - s) * Math.exp(-(t - a) / d);
    const tail = sec - r;
    if (t > tail) v *= 0.5 + 0.5 * Math.cos(Math.PI * Math.min(1, (t - tail) / r));
    return v;
  };
}

/**
 * One oscillator voice, `sec` long.
 *   f        Hz, or t => Hz (see glide / path)
 *   wave     "sine" | "tri" | "soft"
 *   env      an adsr()
 *   vibrato  { rate Hz, cents, delay s }: a slow wobble that eases in after `delay`
 *   fm       { ratio, index: number | t => number }: a sine modulator, for shimmer
 */
function tone({ f, sec, wave = "sine", env = adsr(), vibrato = null, fm = null, gain = 1 }) {
  const out = new Float64Array(frames(sec));
  const hzAt = typeof f === "function" ? f : () => f;
  const indexAt = fm && (typeof fm.index === "function" ? fm.index : () => fm.index);
  const shape = WAVES[wave];
  let phase = 0;
  let mod = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    let hz = hzAt(t);
    if (vibrato) {
      const cents = vibrato.cents * smooth((t - (vibrato.delay ?? 0)) / 0.12);
      hz *= 2 ** ((cents * Math.sin(TAU * vibrato.rate * t)) / 1200);
    }
    let p = phase;
    if (fm) {
      p += indexAt(t) * Math.sin(mod);
      mod = (mod + (TAU * hz * fm.ratio) / SR) % TAU;
    }
    out[i] = gain * env(t, sec) * shape(p);
    phase = (phase + (TAU * hz) / SR) % TAU;
  }
  return out;
}

/** mulberry32: a tiny seeded PRNG, so the "random" noise is the same every run. */
function prng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * A breath of air: white noise through a band-pass whose centre slides from
 * `from` to `to` Hz (a state-variable filter, which stays stable while it
 * moves), shaped by `env`. A low `q` is breathy, a higher one more whistly.
 */
function whoosh({ sec, from, to, q = 1, seed = 1, env = adsr({ a: sec * 0.4, d: sec * 0.25, r: sec * 0.35 }) }) {
  const out = new Float64Array(frames(sec));
  const rnd = prng(seed);
  const centre = glide(from, to, sec);
  const k = 1 / q;
  let s1 = 0;
  let s2 = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    const g = Math.tan((Math.PI * Math.min(centre(t), SR * 0.45)) / SR);
    const a1 = 1 / (1 + g * (g + k));
    const a2 = g * a1;
    const a3 = g * a2;
    const v3 = rnd() * 2 - 1 - s2;
    const v1 = a1 * s1 + a2 * v3;
    const v2 = s2 + a2 * s1 + a3 * v3;
    s1 = 2 * v1 - s1;
    s2 = 2 * v2 - s2;
    out[i] = env(t, sec) * v1 * k; // band-pass, unity gain at the centre
  }
  return out;
}

/** Adds buffers together, all starting at 0. */
function sum(...bufs) {
  const out = new Float64Array(Math.max(...bufs.map((b) => b.length)));
  for (const b of bufs) for (let i = 0; i < b.length; i++) out[i] += b[i];
  return out;
}

/** A track `sec` long with each [start s, buffer, gain] layer added in. */
function mix(sec, layers) {
  const out = new Float64Array(frames(sec));
  for (const [at, buf, gain = 1] of layers) {
    const o = frames(at);
    for (let i = 0; i < buf.length && o + i < out.length; i++) out[o + i] += buf[i] * gain;
  }
  return out;
}

/** `count` repeats of the track, `delay` s apart, each `fb` times quieter (same length). */
function echo(buf, { delay, fb, count }) {
  const out = Float64Array.from(buf);
  const d = frames(delay);
  for (let k = 1; k <= count; k++) {
    const g = fb ** k;
    for (let i = k * d; i < out.length; i++) out[i] += buf[i - k * d] * g;
  }
  return out;
}

/**
 * A small airy room blended in at `wet`: four damped comb filters into two
 * all-passes (Schroeder's design, as in Freeverb). Keeps the track's length.
 */
function air(buf, { wet = 0.16, room = 0.7, damp = 0.4 } = {}) {
  const combs = [1116, 1277, 1422, 1557].map((n) => ({ line: new Float64Array(n), i: 0, lp: 0 }));
  const passes = [556, 441].map((n) => ({ line: new Float64Array(n), i: 0 }));
  const out = new Float64Array(buf.length);
  for (let n = 0; n < buf.length; n++) {
    const x = buf[n];
    let y = 0;
    for (const c of combs) {
      const v = c.line[c.i];
      c.lp = v * (1 - damp) + c.lp * damp;
      c.line[c.i] = x + c.lp * room;
      c.i = (c.i + 1) % c.line.length;
      y += v;
    }
    y *= 0.25;
    for (const p of passes) {
      const v = p.line[p.i];
      p.line[p.i] = y + v * 0.5;
      p.i = (p.i + 1) % p.line.length;
      y = v - y;
    }
    out[n] = x + wet * y;
  }
  return out;
}

/**
 * The last stage of every sound: a DC blocker, a gentle roll-off above 7.5 kHz,
 * raised-cosine fades (1 ms in, 6–80 ms out) so nothing clicks, and a gain
 * that puts the peak at -3 dBFS.
 */
function master(buf) {
  const out = Float64Array.from(buf);
  const hp = 1 - (TAU * 20) / SR;
  let x1 = 0;
  let y1 = 0;
  for (let i = 0; i < out.length; i++) {
    y1 = out[i] - x1 + hp * y1;
    x1 = out[i];
    out[i] = y1;
  }
  const lp = 1 - Math.exp((-TAU * 7500) / SR);
  let z = 0;
  for (let i = 0; i < out.length; i++) out[i] = z += lp * (out[i] - z);

  const fadeIn = frames(0.001);
  const fadeOut = frames(Math.min(0.08, Math.max(0.006, (out.length / SR) * 0.1)));
  for (let i = 0; i < fadeIn; i++) out[i] *= 0.5 - 0.5 * Math.cos((Math.PI * i) / fadeIn);
  for (let k = 0; k < fadeOut; k++) out[out.length - 1 - k] *= 0.5 - 0.5 * Math.cos((Math.PI * k) / fadeOut);

  let peak = 0;
  for (const v of out) peak = Math.max(peak, Math.abs(v));
  if (peak > 0) for (let i = 0; i < out.length; i++) out[i] *= PEAK / peak;
  return out;
}

/** 16-bit mono PCM WAV: the 44-byte RIFF header, then little-endian samples. */
function wav(buf) {
  const data = buf.length * 2;
  const b = Buffer.alloc(44 + data);
  b.write("RIFF", 0, "ascii");
  b.writeUInt32LE(36 + data, 4);
  b.write("WAVE", 8, "ascii");
  b.write("fmt ", 12, "ascii");
  b.writeUInt32LE(16, 16); // fmt chunk size
  b.writeUInt16LE(1, 20); // PCM
  b.writeUInt16LE(1, 22); // mono
  b.writeUInt32LE(SR, 24);
  b.writeUInt32LE(SR * 2, 28); // byte rate
  b.writeUInt16LE(2, 32); // block align
  b.writeUInt16LE(16, 34); // bits per sample
  b.write("data", 36, "ascii");
  b.writeUInt32LE(data, 40);
  for (let i = 0; i < buf.length; i++) {
    b.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(buf[i] * 32767))), 44 + i * 2);
  }
  return b;
}

// ── Instruments ──────────────────────────────────────────────────────────────

/**
 * Mallet on wood: the note, its octave for warmth on small speakers, and a
 * marimba's tuned partial two octaves up that is gone almost at once. `ring`
 * adds a twin a hair sharp, so a held note beats slowly like a struck bar.
 */
function mallet(f, sec, { decay = 0.2, bright = 0.2, release = 0.05, ring = 0 } = {}) {
  const voices = [
    tone({ f, sec, env: adsr({ a: 0.006, d: decay, r: release }) }),
    tone({ f: times(f, 2), sec, env: adsr({ a: 0.005, d: decay * 0.45, r: release }), gain: 0.2 }),
    tone({ f: times(f, 4), sec, env: adsr({ a: 0.003, d: decay * 0.16, r: release }), gain: bright }),
  ];
  if (ring) voices.push(tone({ f: times(f, 1.004), sec, env: adsr({ a: 0.012, d: decay * 1.3, r: release }), gain: ring }));
  return sum(...voices);
}

/**
 * A tap on something solid, the sound of a good key or a lid closing: a click
 * of air, the wood's own short note (`wood` Hz) with a partial above it, and a
 * low body that drops to `f` as it lands.
 */
function thock(f, { wood = 520, sec = 0.16, body = 1, click = 0.3, ring = 0.028, seed = 7 } = {}) {
  return mix(sec, [
    [0, tone({ f: glide(f * 2.2, f, 0.035), sec, wave: "soft", env: adsr({ a: 0.002, d: 0.05, r: 0.04 }) }), body],
    [0, tone({ f: wood, sec: Math.min(sec, 0.12), env: adsr({ a: 0.001, d: ring, r: 0.02 }) }), 0.5],
    [0, tone({ f: wood * 2.76, sec: Math.min(sec, 0.06), env: adsr({ a: 0.001, d: ring * 0.4, r: 0.01 }) }), 0.16],
    [0, whoosh({ sec: 0.014, from: 2600, to: 1200, q: 0.7, seed, env: adsr({ a: 0.0005, d: 0.003, r: 0.005 }) }), click],
  ]);
}

/**
 * A deep bell with a long clean tail: the note and a twin a hair sharp that
 * beats slowly against it, an octave and a twelfth that fade sooner, a glint
 * that is gone at once, and a low thump under the strike.
 */
function bell(f, sec, { decay = 0.45, glint = 0.06, thump = 0.5 } = {}) {
  const r = sec * 0.3;
  return sum(
    tone({ f, sec, env: adsr({ a: 0.004, d: decay, r }) }),
    tone({ f: times(f, 1.003), sec, env: adsr({ a: 0.012, d: decay * 1.2, r }), gain: 0.35 }),
    tone({ f: times(f, 2), sec, env: adsr({ a: 0.004, d: decay * 0.5, r }), gain: 0.26 }),
    tone({ f: times(f, 3), sec, env: adsr({ a: 0.003, d: decay * 0.28, r }), gain: 0.09 }),
    tone({ f: times(f, 4.2), sec, env: adsr({ a: 0.002, d: decay * 0.1, r }), gain: glint }),
    tone({ f: glide(150, 68, 0.03), sec: Math.min(sec, 0.14), wave: "soft", env: adsr({ a: 0.002, d: 0.04, r: 0.04 }), gain: thump }),
  );
}

/** Bubble: a sine that leaps up to f in a few ms, then fades. */
const bubble = (f, sec, { from = 0.6, rise = 0.03, decay = 0.04 } = {}) =>
  tone({ f: glide(f * from, f, rise), sec, wave: "soft", env: adsr({ a: 0.005, d: decay, r: 0.015 }) });

/** A darker, slightly larger room than air()'s own. */
const room = (buf, wet = 0.18) => air(buf, { wet, room: 0.76, damp: 0.6 });

// ── Recipes ──────────────────────────────────────────────────────────────────
// Few notes, and low ones. What you touch answers with a tap on wood (thock);
// what asks for you or tells you something rings a deep bell once or twice and
// lets it fade; only Ciel's own moods (a yawn, a sigh, a wobble) still sing.

const SOUNDS = {
  // peek: a light tap and a small open fifth behind it, like a knock at the door (G4, D5)
  peek: () =>
    room(mix(0.6, [
      [0.0, thock(110, { wood: 700, sec: 0.1, body: 0.5 }), 0.7],
      [0.04, bell(note("G4"), 0.55, { decay: 0.16, thump: 0 }), 0.45],
      [0.055, bell(note("D5"), 0.5, { decay: 0.13, thump: 0 }), 0.25],
    ]), 0.2),

  // open: a lid lifting: two taps a moment apart, the second higher, each leaving a low note behind (C4, G4)
  open: () =>
    room(mix(0.5, [
      [0.0, whoosh({ sec: 0.2, from: 250, to: 1100, q: 0.8, seed: 21 }), 0.1],
      [0.0, thock(78, { wood: 420 }), 0.85],
      [0.005, bell(note("C4"), 0.45, { decay: 0.13, thump: 0 }), 0.3],
      [0.075, thock(105, { wood: 640, body: 0.6, seed: 8 }), 0.7],
      [0.08, bell(note("G4"), 0.42, { decay: 0.14, thump: 0 }), 0.3],
    ]), 0.2),

  // close: the lid coming down: the same two the other way, the last one deep (G4, C4 over a low C)
  close: () =>
    room(mix(0.5, [
      [0.0, whoosh({ sec: 0.18, from: 1100, to: 250, q: 0.8, seed: 22 }), 0.08],
      [0.0, thock(105, { wood: 640, body: 0.5, seed: 8 }), 0.6],
      [0.005, bell(note("G4"), 0.3, { decay: 0.09, thump: 0 }), 0.25],
      [0.075, thock(66, { wood: 380, sec: 0.2 }), 0.95],
      [0.08, bell(note("C3"), 0.42, { decay: 0.16, glint: 0.02, thump: 0 }), 0.4],
    ]), 0.2),

  // hover: a fingertip on wood, barely there, with a breath of a note in it (G4)
  hover: () =>
    mix(0.09, [
      [0.0, thock(170, { wood: 900, sec: 0.05, body: 0.25, click: 0.18, ring: 0.012 }), 0.9],
      [0.0, bell(note("G4"), 0.09, { decay: 0.03, thump: 0 }), 0.2],
    ]),

  // blip: one clean tap, like a good key going down, on a short low note (C4)
  blip: () =>
    room(mix(0.22, [
      [0.0, thock(115, { wood: 600, sec: 0.1, body: 0.8 }), 0.9],
      [0.0, bell(note("C4"), 0.22, { decay: 0.06, thump: 0 }), 0.3],
    ]), 0.12),

  // slap: a pillowy "puff", like patting a cushion: a deep soft thump and a breath
  slap: () =>
    room(mix(0.22, [
      [0.0, tone({ f: glide(210, 70, 0.08), sec: 0.18, wave: "soft", env: adsr({ a: 0.004, d: 0.05, r: 0.04 }) }), 0.9],
      [0.0, whoosh({ sec: 0.12, from: 900, to: 300, q: 0.7, seed: 31, env: adsr({ a: 0.004, d: 0.03, r: 0.04 }) }), 0.4],
    ]), 0.1),

  // annoyed: a pouty "hmph": two dull knocks, and a low note that sags under the second (B3 → A3)
  annoyed: () =>
    room(mix(0.5, [
      [0.0, thock(84, { wood: 300, sec: 0.12, click: 0.15 }), 0.7],
      [0.12, thock(70, { wood: 250, sec: 0.18, click: 0.12, seed: 9 }), 0.85],
      [0.12, bell(glide(note("B3"), note("A3"), 0.25), 0.38, { decay: 0.14, glint: 0, thump: 0 }), 0.45],
    ]), 0.14),

  // dizzy: a woozy wobble: two voices swimming in pitch, out of step, sinking C5 → G4
  dizzy: () => {
    const sink = glide(note("C5"), note("G4"), 0.6);
    const swim = (phase) => (t) => sink(t) * 2 ** ((0.9 * smooth(t / 0.25) * Math.sin(TAU * 6 * t + phase)) / 12);
    const under = swim(Math.PI);
    return room(mix(0.7, [
      [0.0, tone({ f: swim(0), sec: 0.7, wave: "soft", env: adsr({ a: 0.03, d: 0.25, s: 0.45, r: 0.2 }) }), 0.6],
      [0.0, tone({ f: (t) => under(t) * 0.5, sec: 0.7, wave: "tri", env: adsr({ a: 0.05, d: 0.25, s: 0.45, r: 0.2 }) }), 0.4],
    ]));
  },

  // greet: a warm "hello": a tap, then an open chord rolled upward and left to ring (G2, D3, B3, G4)
  greet: () =>
    room(mix(1.2, [
      [0.0, thock(76, { wood: 420 }), 0.55],
      [0.02, bell(note("G2"), 1.15, { decay: 0.5, glint: 0.02 }), 0.6],
      [0.06, bell(note("D3"), 1.1, { decay: 0.46, thump: 0 }), 0.55],
      [0.1, bell(note("B3"), 1.05, { decay: 0.42, thump: 0 }), 0.5],
      [0.14, bell(note("G4"), 1.0, { decay: 0.34, thump: 0 }), 0.3],
    ]), 0.22),

  // work: knuckles on the desk, three of them, the last a little higher and ringing on (C3, G3)
  work: () =>
    room(mix(0.9, [
      [0.0, thock(88, { wood: 380, seed: 12 }), 0.7],
      [0.17, thock(88, { wood: 380, seed: 13 }), 0.55],
      [0.34, thock(96, { wood: 480, sec: 0.2, seed: 14 }), 0.85],
      [0.345, bell(note("C3"), 0.55, { decay: 0.2, glint: 0.02, thump: 0 }), 0.4],
      [0.355, bell(note("G3"), 0.5, { decay: 0.18, thump: 0 }), 0.3],
    ]), 0.2),

  // finish: done. One tap, then a wide low chord rung once and left to fade (C3, G3, E4)
  finish: () =>
    room(mix(1.4, [
      [0.0, thock(72, { wood: 400 }), 0.6],
      [0.03, bell(note("C3"), 1.35, { decay: 0.55, glint: 0.02 }), 0.7],
      [0.045, bell(note("G3"), 1.3, { decay: 0.5, thump: 0 }), 0.55],
      [0.06, bell(note("E4"), 1.25, { decay: 0.42, thump: 0 }), 0.5],
    ]), 0.22),

  // error: two dull knocks on something that does not ring, over a low minor third sinking away (E3, G3)
  error: () =>
    room(mix(1.0, [
      [0.0, thock(64, { wood: 230, sec: 0.2, click: 0.15, ring: 0.02 }), 0.9],
      [0.16, thock(58, { wood: 205, sec: 0.22, click: 0.12, ring: 0.02, seed: 9 }), 0.9],
      [0.16, bell(glide(note("E3"), note("E3") * 0.97, 0.6), 0.84, { decay: 0.32, glint: 0, thump: 0 }), 0.5],
      [0.17, bell(glide(note("G3"), note("G3") * 0.97, 0.6), 0.8, { decay: 0.28, glint: 0, thump: 0 }), 0.35],
    ]), 0.2),

  // approval: he needs you: a tap, a low fifth, and a bell above it that waits for an answer (A2, E3, then E4)
  approval: () =>
    room(mix(1.4, [
      [0.0, thock(74, { wood: 420 }), 0.55],
      [0.02, bell(note("A2"), 1.3, { decay: 0.5, glint: 0.02 }), 0.6],
      [0.035, bell(note("E3"), 1.25, { decay: 0.46, thump: 0 }), 0.5],
      [0.22, bell(note("E4"), 1.15, { decay: 0.42, thump: 0 }), 0.6],
      [0.235, bell(note("B4"), 1.0, { decay: 0.3, thump: 0 }), 0.2],
    ]), 0.22),

  // question: a tap, a low fifth, and one bell that lifts at its tail, the way a voice does when it asks (D3, A3, D4 → E4)
  question: () =>
    room(mix(1.3, [
      [0.0, thock(86, { wood: 480 }), 0.5],
      [0.02, bell(note("D3"), 1.2, { decay: 0.46, glint: 0.02 }), 0.55],
      [0.035, bell(note("A3"), 1.15, { decay: 0.42, thump: 0 }), 0.45],
      [0.05, bell(path([[0, note("D4")], [0.2, note("D4")], [0.6, note("E4")]]), 1.2, { decay: 0.44, thump: 0 }), 0.7],
    ]), 0.22),

  // approve: yes. A firm tap and a bright open fifth on top of it, shorter than finish (C4, G4)
  approve: () =>
    room(mix(0.8, [
      [0.0, thock(92, { wood: 560 }), 0.85],
      [0.012, bell(note("C4"), 0.75, { decay: 0.26, thump: 0 }), 0.55],
      [0.024, bell(note("G4"), 0.7, { decay: 0.22, thump: 0 }), 0.45],
    ]), 0.2),

  // gulp: a round "gloop": a note dipping down, then a little bubble back up
  gulp: () =>
    room(mix(0.34, [
      [0.0, tone({ f: glide(380, 150, 0.11), sec: 0.15, wave: "soft", env: adsr({ a: 0.006, d: 0.07, r: 0.04 }) }), 0.85],
      [0.14, bubble(430, 0.12, { from: 0.45, rise: 0.04, decay: 0.05 }), 0.5],
    ]), 0.12),

  // tick: a tiny dry tick, one for each 10 % of an upload
  tick: () => thock(210, { wood: 1100, sec: 0.04, body: 0.2, click: 0.25, ring: 0.008 }),

  // send: let go: a tap, a short breath of air rising away, and a small fifth where it lands (A4, E5)
  send: () =>
    room(mix(0.7, [
      [0.0, thock(98, { wood: 520 }), 0.8],
      [0.01, whoosh({ sec: 0.26, from: 300, to: 2200, q: 1.0, seed: 41 }), 0.26],
      [0.2, bell(note("A4"), 0.48, { decay: 0.14, thump: 0 }), 0.3],
      [0.215, bell(note("E5"), 0.44, { decay: 0.11, thump: 0 }), 0.14],
    ]), 0.2),

  // love: a warm "aww": two soft chords leaning into each other (G3 + E4, then C4 + A4)
  love: () =>
    room(mix(1.2, [
      [0.0, bell(note("G3"), 0.9, { decay: 0.34, glint: 0.02, thump: 0.25 }), 0.6],
      [0.015, bell(note("E4"), 0.85, { decay: 0.3, glint: 0.02, thump: 0 }), 0.5],
      [0.26, bell(note("C4"), 0.94, { decay: 0.4, glint: 0.02, thump: 0 }), 0.55],
      [0.275, bell(note("A4"), 0.9, { decay: 0.34, glint: 0.03, thump: 0 }), 0.4],
    ]), 0.24),

  // pop: a bubble popping: a note that leaps up in 25 ms, on a click
  pop: () =>
    mix(0.1, [
      [0.0, thock(130, { wood: 760, sec: 0.07, body: 0.4, ring: 0.014 }), 0.5],
      [0.0, bubble(note("E5"), 0.095, { from: 0.4, rise: 0.025, decay: 0.03 }), 0.8],
    ]),

  // proud: a slow strum of low bells, bottom to top, with a glint on the last (C3, G3, C4, E4)
  proud: () =>
    room(mix(1.3, [
      [0.0, bell(note("C3"), 1.25, { decay: 0.5, glint: 0.02 }), 0.65],
      [0.07, bell(note("G3"), 1.15, { decay: 0.46, thump: 0 }), 0.55],
      [0.14, bell(note("C4"), 1.1, { decay: 0.42, thump: 0 }), 0.5],
      [0.21, bell(note("E4"), 1.05, { decay: 0.4, glint: 0.14, thump: 0 }), 0.5],
    ]), 0.22),

  // wink: a quick tap and a small fifth flicked after it (G4, D5)
  wink: () =>
    room(mix(0.5, [
      [0.0, thock(140, { wood: 820, sec: 0.08, body: 0.4 }), 0.6],
      [0.05, bell(note("G4"), 0.44, { decay: 0.12, thump: 0 }), 0.5],
      [0.065, bell(note("D5"), 0.4, { decay: 0.1, thump: 0 }), 0.25],
    ]), 0.2),

  // yawn: a slow, sleepy glide that stretches up a little, then sighs down (E4, G4, B3)
  yawn: () =>
    room(mix(0.85, [
      [0.0, tone({
        f: path([[0, note("E4")], [0.22, note("G4")], [0.78, note("B3")]]),
        sec: 0.82,
        wave: "soft",
        env: adsr({ a: 0.09, d: 0.4, s: 0.45, r: 0.28 }),
        vibrato: { rate: 4.5, cents: 10, delay: 0.25 },
      }), 0.75],
      [0.05, whoosh({ sec: 0.7, from: 800, to: 300, q: 0.8, seed: 51, env: adsr({ a: 0.2, d: 0.3, s: 0.3, r: 0.25 }) }), 0.14],
    ]), 0.2),

  // attach: click-clack, a magnet finding its place: a light tap, then a deeper one that rings a little (C3, G3)
  attach: () =>
    room(mix(0.45, [
      [0.0, thock(150, { wood: 780, sec: 0.07, body: 0.35, ring: 0.014 }), 0.6],
      [0.06, thock(84, { wood: 470, sec: 0.17, seed: 9 }), 0.9],
      [0.065, bell(note("C3"), 0.38, { decay: 0.13, glint: 0.02, thump: 0 }), 0.35],
      [0.07, bell(note("G3"), 0.34, { decay: 0.11, thump: 0 }), 0.25],
    ]), 0.18),

  // think: three soft low bells stepping up, slow and pensive, each left hanging (A3, C4, E4)
  think: () =>
    room(mix(1.1, [
      [0.0, bell(note("A3"), 0.5, { decay: 0.18, glint: 0.02, thump: 0.2 }), 0.5],
      [0.2, bell(note("C4"), 0.5, { decay: 0.18, glint: 0.02, thump: 0 }), 0.55],
      [0.4, bell(note("E4"), 0.7, { decay: 0.26, glint: 0.02, thump: 0 }), 0.6],
    ]), 0.24),

  // search: a gentle scanning sweep up from A4 and back, echoing, like a soft sonar
  search: () => {
    const scan = (t) => note("A4") * 2 ** ((5 * (0.5 - 0.5 * Math.cos((TAU * t) / 0.42))) / 12);
    const sweep = tone({ f: scan, sec: 0.42, wave: "soft", env: adsr({ a: 0.04, d: 0.2, s: 0.5, r: 0.12 }) });
    return room(echo(mix(0.66, [[0.0, sweep, 0.8]]), { delay: 0.12, fb: 0.32, count: 2 }), 0.18);
  },

  // rate: a tired wind-down: three low bells stepping lower and slower (E4, C4, A3), the last sagging
  rate: () =>
    room(mix(1.2, [
      [0.0, bell(note("E4"), 0.4, { decay: 0.14, glint: 0, thump: 0 }), 0.6],
      [0.2, bell(note("C4"), 0.45, { decay: 0.16, glint: 0, thump: 0 }), 0.6],
      [0.46, bell(glide(note("A3"), note("A3") * 0.94, 0.5), 0.74, { decay: 0.3, glint: 0, thump: 0.3 }), 0.7],
    ]), 0.2),

  // sleep: lights out: two slow bells settling down a fifth over a hush of breath (D4, then G3 with a low G under it)
  sleep: () =>
    room(mix(1.6, [
      [0.0, whoosh({ sec: 1.2, from: 700, to: 250, q: 0.7, seed: 71, env: adsr({ a: 0.4, d: 0.4, s: 0.3, r: 0.4 }) }), 0.08],
      [0.0, bell(note("D4"), 1.1, { decay: 0.44, glint: 0, thump: 0 }), 0.5],
      [0.4, bell(note("G3"), 1.2, { decay: 0.55, glint: 0, thump: 0 }), 0.6],
      [0.42, bell(note("G2"), 1.15, { decay: 0.55, glint: 0, thump: 0.2 }), 0.4],
    ]), 0.26),

  // greeting: the launch, lined up with src/cloud/greeting.ts: a falling breath as Ciel
  // drops in, a glint at the burst ring (0.45 s), a deep landing (0.58) and bounce
  // (0.72), a little spring (1.3), then a wave tune E D E G A that comes home to G
  // over a C major chord. A quiet low C/G pad holds it all together.
  greeting: () => {
    const tune = [[1.45, "E5", 0.18], [1.6, "D5", 0.12], [1.7, "E5", 0.18], [1.85, "G5", 0.16], [2.0, "A5", 0.12]];
    const pad = sum(
      tone({ f: note("C3"), sec: 1.9, wave: "soft", env: adsr({ a: 0.5, d: 1.0, s: 0.5, r: 0.6 }), gain: 0.8 }),
      tone({ f: note("C4"), sec: 1.9, wave: "soft", env: adsr({ a: 0.5, d: 1.0, s: 0.5, r: 0.6 }) }),
      tone({ f: note("G4"), sec: 1.9, env: adsr({ a: 0.6, d: 1.0, s: 0.5, r: 0.6 }), gain: 0.7 }),
    );
    const last = { decay: 0.3, release: 0.15, ring: 0.3 };
    return room(mix(2.5, [
      [0.0, whoosh({ sec: 0.62, from: 1800, to: 350, q: 0.8, seed: 81, env: adsr({ a: 0.38, d: 0.12, s: 0.2, r: 0.18 }) }), 0.28],
      [0.45, mallet(note("G5"), 0.3, { decay: 0.08, bright: 0.1 }), 0.18],
      [0.58, thock(64, { wood: 380, sec: 0.22 }), 0.9],
      [0.72, mallet(note("G4"), 0.3, { decay: 0.12 }), 0.5],
      [1.28, tone({ f: glide(note("C4"), note("G4"), 0.1), sec: 0.24, wave: "soft", env: adsr({ a: 0.01, d: 0.08, r: 0.06 }) }), 0.45],
      [0.6, pad, 0.1],
      ...tune.map(([at, n, sec]) => [at, bell(note(n), sec + 0.2, { decay: sec * 0.8, thump: 0 }), 0.5]),
      [2.1, mallet(note("G5"), 0.4, last), 0.5],
      [2.1, bell(note("C3"), 0.4, { decay: 0.3 }), 0.5],
      [2.1, bell(note("G3"), 0.4, { decay: 0.3, thump: 0 }), 0.4],
      [2.1, bell(note("E4"), 0.4, { decay: 0.3, thump: 0 }), 0.4],
    ]), 0.2);
  },
};

// ── Write ────────────────────────────────────────────────────────────────────

const soundTs = readFileSync(join(ROOT, "src", "core", "sound.ts"), "utf8");
const declared = /SOUND_NAMES\s*=\s*\[([^\]]*)\]/.exec(soundTs);
if (!declared) throw new Error("SOUND_NAMES not found in src/core/sound.ts");
const expected = [...declared[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
const missing = expected.filter((n) => !(n in SOUNDS));
const extra = Object.keys(SOUNDS).filter((n) => !expected.includes(n));
if (missing.length || extra.length) {
  throw new Error(`recipes out of step with SOUND_NAMES: missing [${missing}] extra [${extra}]`);
}

mkdirSync(OUT, { recursive: true });
for (const name of expected) {
  const samples = master(SOUNDS[name]());
  writeFileSync(join(OUT, `${name}.wav`), wav(samples));
  console.log(`${name.padEnd(9)} ${Math.round((samples.length / SR) * 1000)} ms`);
}
console.log(`${expected.length} sounds → ${OUT}`);
