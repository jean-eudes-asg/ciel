// Ciel's 29 sounds, synthesized from code. No samples and no dependencies: each
// sound is a small recipe of soft sine/triangle tones, smooth envelopes and a
// breath of filtered noise, written as 16-bit mono 44.1 kHz WAV into sounds/.
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
 * The last stage of every sound: a DC blocker, a gentle roll-off above 9 kHz,
 * raised-cosine fades (5 ms in, 6–80 ms out) so nothing clicks, and a gain
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
  const lp = 1 - Math.exp((-TAU * 9000) / SR);
  let z = 0;
  for (let i = 0; i < out.length; i++) out[i] = z += lp * (out[i] - z);

  const fadeIn = frames(0.005);
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

/** Glassy bell: a sine with a quick FM shimmer on the strike and a faint octave above. */
function chime(f, sec, { decay = 0.18, shimmer = 0.7, release = 0.03 } = {}) {
  return sum(
    tone({ f, sec, env: adsr({ a: 0.004, d: decay, r: release }), fm: { ratio: 2, index: (t) => shimmer * Math.exp(-t / 0.04) } }),
    tone({ f: times(f, 2), sec, env: adsr({ a: 0.003, d: decay * 0.35, r: release }), gain: 0.12 }),
  );
}

/** Round pluck: a soft triangle that dies away fast. */
const pluck = (f, sec, decay = 0.05) => tone({ f, sec, wave: "tri", env: adsr({ a: 0.005, d: decay, r: 0.015 }) });

/** Bubble: a sine that leaps up to f in a few ms, then fades. */
const bubble = (f, sec, { from = 0.6, rise = 0.03, decay = 0.04 } = {}) =>
  tone({ f: glide(f * from, f, rise), sec, env: adsr({ a: 0.004, d: decay, r: 0.015 }) });

/** Warm held note: the soft wave with a slow vibrato and a twin a hair sharp. */
const warm = (f, sec) =>
  sum(
    tone({ f, sec, wave: "soft", env: adsr({ a: 0.04, d: 0.22, s: 0.35, r: 0.16 }), vibrato: { rate: 5, cents: 14, delay: 0.08 } }),
    tone({ f: f * 1.003, sec, env: adsr({ a: 0.06, d: 0.22, s: 0.35, r: 0.16 }), gain: 0.35 }),
  );

// ── Recipes ──────────────────────────────────────────────────────────────────
// Mostly C major pentatonic, mostly between C5 and C7: high, light, airy.

const SOUNDS = {
  // peek: a breath of air lifting into a little two-note "peekaboo" (G5, C6)
  peek: () =>
    air(mix(0.36, [
      [0.0, whoosh({ sec: 0.2, from: 800, to: 2800, q: 1.4, seed: 11 }), 0.3],
      [0.05, chime(note("G5"), 0.12, { decay: 0.05 }), 0.55],
      [0.12, chime(note("C6"), 0.22, { decay: 0.09 }), 0.8],
    ])),

  // open: an airy upward swish carrying a soft note that rises E5 → B5
  open: () =>
    air(mix(0.36, [
      [0.0, whoosh({ sec: 0.28, from: 600, to: 3000, q: 0.9, seed: 21 }), 0.35],
      [0.02, tone({ f: glide(note("E5"), note("B5"), 0.14), sec: 0.3, wave: "soft", env: adsr({ a: 0.025, d: 0.1, s: 0.15, r: 0.08 }) }), 0.6],
    ])),

  // close: the same breath folding back down, B5 → E5
  close: () =>
    air(mix(0.34, [
      [0.0, whoosh({ sec: 0.26, from: 3000, to: 600, q: 0.9, seed: 22 }), 0.35],
      [0.02, tone({ f: glide(note("B5"), note("E5"), 0.14), sec: 0.28, wave: "soft", env: adsr({ a: 0.02, d: 0.09, s: 0.15, r: 0.08 }) }), 0.6],
    ])),

  // hover: a feather-light high tap, barely there (E6)
  hover: () => tone({ f: note("E6"), sec: 0.06, env: adsr({ a: 0.004, d: 0.014, r: 0.012 }), fm: { ratio: 2, index: 0.3 } }),

  // blip: a round soft blip with a tiny downward lilt onto A5
  blip: () => tone({ f: glide(note("A5") * 1.06, note("A5"), 0.03), sec: 0.09, wave: "tri", env: adsr({ a: 0.005, d: 0.03, r: 0.02 }) }),

  // slap: a pillowy "puff", like patting a cushion: a soft low thump and a breath
  slap: () =>
    air(mix(0.2, [
      [0.0, tone({ f: glide(300, 120, 0.07), sec: 0.16, wave: "soft", env: adsr({ a: 0.004, d: 0.045, r: 0.04 }) }), 0.9],
      [0.0, whoosh({ sec: 0.12, from: 1600, to: 500, q: 0.7, seed: 31, env: adsr({ a: 0.004, d: 0.03, r: 0.04 }) }), 0.5],
    ]), { wet: 0.1 }),

  // annoyed: a pouty little "hmph": two short low notes, the second sagging down
  annoyed: () =>
    air(mix(0.34, [
      [0.0, tone({ f: note("B4"), sec: 0.11, wave: "soft", env: adsr({ a: 0.008, d: 0.05, s: 0.3, r: 0.03 }) }), 0.7],
      [0.12, tone({ f: glide(note("F#4"), note("F#4") * 0.94, 0.2), sec: 0.2, wave: "soft", env: adsr({ a: 0.01, d: 0.08, s: 0.3, r: 0.07 }) }), 0.8],
    ]), { wet: 0.1 }),

  // dizzy: a woozy wobble: two voices swimming in pitch, out of step, sinking C6 → G5
  dizzy: () => {
    const sink = glide(note("C6"), note("G5"), 0.6);
    const swim = (phase) => (t) => sink(t) * 2 ** ((0.9 * smooth(t / 0.25) * Math.sin(TAU * 6.5 * t + phase)) / 12);
    const under = swim(Math.PI);
    return air(mix(0.7, [
      [0.0, tone({ f: swim(0), sec: 0.7, env: adsr({ a: 0.03, d: 0.25, s: 0.45, r: 0.2 }) }), 0.6],
      [0.0, tone({ f: (t) => under(t) * 0.75, sec: 0.7, wave: "tri", env: adsr({ a: 0.05, d: 0.25, s: 0.45, r: 0.2 }) }), 0.35],
    ]));
  },

  // greet: a bright "hi!": a quick upward chirp (G5 → C6) and a sparkly answer on E6
  greet: () =>
    air(mix(0.44, [
      [0.0, tone({ f: glide(note("G5"), note("C6"), 0.05), sec: 0.11, wave: "tri", env: adsr({ a: 0.006, d: 0.05, r: 0.03 }) }), 0.6],
      [0.09, chime(note("E6"), 0.32, { decay: 0.12 }), 0.75],
      [0.09, chime(note("C6"), 0.3, { decay: 0.1 }), 0.3],
    ]), { wet: 0.2 }),

  // work: a light, steady three-beat pulse on C6 with a faint fifth above
  work: () => {
    const beat = () =>
      sum(
        tone({ f: note("C6"), sec: 0.2, env: adsr({ a: 0.008, d: 0.05, r: 0.04 }) }),
        tone({ f: note("G6"), sec: 0.2, env: adsr({ a: 0.008, d: 0.03, r: 0.04 }), gain: 0.18 }),
      );
    return air(mix(0.68, [[0.0, beat(), 0.75], [0.18, beat(), 0.6], [0.36, beat(), 0.85]]), { wet: 0.2 });
  },

  // finish: a happy arpeggio climbing C5 E5 G5 C6, the top note ringing out
  finish: () =>
    air(mix(0.72, [
      [0.0, chime(note("C5"), 0.22, { decay: 0.1 }), 0.55],
      [0.075, chime(note("E5"), 0.22, { decay: 0.1 }), 0.6],
      [0.15, chime(note("G5"), 0.25, { decay: 0.11 }), 0.65],
      [0.225, chime(note("C6"), 0.48, { decay: 0.22, release: 0.12 }), 0.8],
      [0.26, chime(note("E6"), 0.4, { decay: 0.12, release: 0.1 }), 0.18],
    ]), { wet: 0.2 }),

  // error: a soft, low "uh-oh": G4 falling to E4, warm and rounded, no edge
  error: () =>
    air(mix(0.6, [
      [0.0, tone({ f: note("G4"), sec: 0.22, wave: "soft", env: adsr({ a: 0.012, d: 0.12, s: 0.2, r: 0.06 }) }), 0.75],
      [0.17, tone({ f: glide(note("E4"), note("E4") * 0.985, 0.4), sec: 0.42, wave: "soft", env: adsr({ a: 0.015, d: 0.18, s: 0.1, r: 0.14 }) }), 0.85],
    ])),

  // approval: an attentive chime rising a fourth (E5 → A5): noticeable, never startling
  approval: () =>
    air(mix(0.64, [
      [0.0, chime(note("E5"), 0.3, { decay: 0.14 }), 0.7],
      [0.14, chime(note("A5"), 0.5, { decay: 0.22, release: 0.1 }), 0.85],
    ]), { wet: 0.2 }),

  // question: a curious pair rising a fifth (G5 → D6), the second note lifting at its tail
  question: () =>
    air(mix(0.64, [
      [0.0, chime(note("G5"), 0.28, { decay: 0.12 }), 0.7],
      [0.14, chime(path([[0, note("D6")], [0.12, note("D6")], [0.36, note("Eb6")]]), 0.5, { decay: 0.22, release: 0.1 }), 0.85],
    ]), { wet: 0.2 }),

  // approve: a quick, bright "ding-ding" up a major third (C6, E6): yes, done
  approve: () =>
    air(mix(0.44, [
      [0.0, chime(note("C6"), 0.18, { decay: 0.07 }), 0.7],
      [0.07, chime(note("E6"), 0.36, { decay: 0.14, release: 0.08 }), 0.85],
    ]), { wet: 0.18 }),

  // gulp: a round "gloop": a sine dipping down, then a little bubble back up
  gulp: () =>
    air(mix(0.34, [
      [0.0, tone({ f: glide(560, 230, 0.11), sec: 0.15, wave: "soft", env: adsr({ a: 0.006, d: 0.07, r: 0.04 }) }), 0.85],
      [0.14, bubble(640, 0.12, { from: 0.45, rise: 0.04, decay: 0.05 }), 0.5],
    ]), { wet: 0.12 }),

  // tick: a tiny dry glass tick, one for each 10 % of an upload (A6)
  tick: () =>
    tone({ f: note("A6"), sec: 0.045, env: adsr({ a: 0.003, d: 0.009, r: 0.01 }), fm: { ratio: 3, index: (t) => 0.6 * Math.exp(-t / 0.006) } }),

  // send: a breezy swoosh carrying a note up A5 → E6, like a paper plane taking off
  send: () =>
    air(mix(0.4, [
      [0.0, whoosh({ sec: 0.32, from: 700, to: 4200, q: 1.1, seed: 41 }), 0.4],
      [0.03, tone({ f: glide(note("A5"), note("E6"), 0.18), sec: 0.26, wave: "soft", env: adsr({ a: 0.02, d: 0.09, s: 0.1, r: 0.07 }) }), 0.55],
      [0.2, chime(note("A6"), 0.18, { decay: 0.06 }), 0.15],
    ]), { wet: 0.2 }),

  // love: a warm "aww": two soft notes rising a sixth (G5 → E6) with a slow vibrato
  love: () => air(mix(0.7, [[0.0, warm(note("G5"), 0.36), 0.6], [0.16, warm(note("E6"), 0.54), 0.75]]), { wet: 0.22 }),

  // pop: a bubble popping: a sine that leaps up to E6 in 25 ms
  pop: () => bubble(note("E6"), 0.085, { from: 0.4, rise: 0.025, decay: 0.03 }),

  // proud: a tiny "ta-daa": a quick pickup into a rolled C major chord with a sparkle on top
  proud: () =>
    air(mix(0.66, [
      [0.0, pluck(note("G5"), 0.1, 0.04), 0.5],
      [0.09, chime(note("C6"), 0.52, { decay: 0.22, release: 0.12 }), 0.55],
      [0.11, chime(note("E6"), 0.5, { decay: 0.2, release: 0.12 }), 0.5],
      [0.13, chime(note("G6"), 0.48, { decay: 0.18, release: 0.12 }), 0.45],
      [0.16, chime(note("C7"), 0.3, { decay: 0.08 }), 0.15],
    ]), { wet: 0.2 }),

  // wink: a cheeky "tink" that flicks upward (C6 → G6) with a wee sparkle
  wink: () =>
    air(mix(0.28, [
      [0.0, tone({ f: glide(note("C6"), note("G6"), 0.045), sec: 0.12, wave: "tri", env: adsr({ a: 0.005, d: 0.045, r: 0.03 }) }), 0.6],
      [0.07, chime(note("D7"), 0.16, { decay: 0.05, shimmer: 0.3 }), 0.25],
    ]), { wet: 0.18 }),

  // yawn: a slow, sleepy glide that stretches up a little, then sighs down (E5, G5, B4)
  yawn: () =>
    air(mix(0.85, [
      [0.0, tone({
        f: path([[0, note("E5")], [0.22, note("G5")], [0.78, note("B4")]]),
        sec: 0.82,
        wave: "soft",
        env: adsr({ a: 0.09, d: 0.4, s: 0.45, r: 0.28 }),
        vibrato: { rate: 4.5, cents: 10, delay: 0.25 },
      }), 0.75],
      [0.05, whoosh({ sec: 0.7, from: 1400, to: 500, q: 0.8, seed: 51, env: adsr({ a: 0.2, d: 0.3, s: 0.3, r: 0.25 }) }), 0.18],
    ]), { wet: 0.2 }),

  // attach: a soft two-step "click-clack", like a paper clip snapping on (D6, G6)
  attach: () =>
    air(mix(0.2, [
      [0.0, pluck(note("D6"), 0.06, 0.016), 0.6],
      [0.0, whoosh({ sec: 0.02, from: 3000, to: 2500, q: 1.5, seed: 61, env: adsr({ a: 0.002, d: 0.005, r: 0.008 }) }), 0.25],
      [0.055, pluck(note("G6"), 0.12, 0.03), 0.7],
    ]), { wet: 0.12 }),

  // think: three soft thought-bubbles floating up, slow and pensive (A5, C6, E6)
  think: () =>
    air(mix(0.62, [
      [0.0, bubble(note("A5"), 0.16, { from: 0.7, rise: 0.04, decay: 0.06 }), 0.55],
      [0.18, bubble(note("C6"), 0.16, { from: 0.7, rise: 0.04, decay: 0.06 }), 0.65],
      [0.36, bubble(note("E6"), 0.24, { from: 0.7, rise: 0.04, decay: 0.09 }), 0.75],
    ]), { wet: 0.22 }),

  // search: a gentle scanning sweep up from A5 and back, echoing, like a soft sonar
  search: () => {
    const scan = (t) => note("A5") * 2 ** ((5 * (0.5 - 0.5 * Math.cos((TAU * t) / 0.42))) / 12);
    const sweep = tone({ f: scan, sec: 0.42, wave: "soft", env: adsr({ a: 0.04, d: 0.2, s: 0.5, r: 0.12 }) });
    return air(echo(mix(0.66, [[0.0, sweep, 0.8]]), { delay: 0.12, fb: 0.32, count: 2 }), { wet: 0.18 });
  },

  // rate: a tired little wind-down: three notes stepping lower and slower (E5, C5, A4), the last sagging
  rate: () =>
    air(mix(0.7, [
      [0.0, tone({ f: note("E5"), sec: 0.14, wave: "soft", env: adsr({ a: 0.01, d: 0.06, s: 0.2, r: 0.04 }) }), 0.7],
      [0.16, tone({ f: note("C5"), sec: 0.17, wave: "soft", env: adsr({ a: 0.012, d: 0.07, s: 0.2, r: 0.05 }) }), 0.65],
      [0.38, tone({ f: glide(note("A4"), note("A4") * 0.95, 0.3), sec: 0.3, wave: "soft", env: adsr({ a: 0.015, d: 0.12, s: 0.2, r: 0.1 }) }), 0.7],
    ])),

  // sleep: a slow, dreamy glide drifting down D6 → G5 over a hush of breath, a fifth below humming along
  sleep: () =>
    air(mix(1.0, [
      [0.0, tone({
        f: glide(note("D6"), note("G5"), 0.85),
        sec: 0.98,
        wave: "soft",
        env: adsr({ a: 0.14, d: 0.45, s: 0.4, r: 0.32 }),
        vibrato: { rate: 4, cents: 8, delay: 0.3 },
      }), 0.6],
      [0.04, tone({ f: glide(note("G5"), note("C5"), 0.85), sec: 0.94, env: adsr({ a: 0.18, d: 0.45, s: 0.4, r: 0.3 }) }), 0.3],
      [0.0, whoosh({ sec: 0.95, from: 1200, to: 400, q: 0.7, seed: 71, env: adsr({ a: 0.3, d: 0.35, s: 0.3, r: 0.3 }) }), 0.15],
    ]), { wet: 0.22 }),

  // greeting: the launch, lined up with src/cloud/greeting.ts: a falling breath as Ciel
  // drops in, a sparkle at the burst ring (0.45 s), a soft landing (0.58) and bounce
  // (0.72), a little spring (1.3), then a wave tune E D E G A that comes home to G
  // over a C major chord. A quiet C/G pad holds it all together.
  greeting: () => {
    const tune = [[1.45, "E6", 0.18], [1.6, "D6", 0.12], [1.7, "E6", 0.18], [1.85, "G6", 0.16], [2.0, "A6", 0.12]];
    const pad = sum(
      tone({ f: note("C5"), sec: 1.9, wave: "soft", env: adsr({ a: 0.5, d: 1.0, s: 0.5, r: 0.6 }) }),
      tone({ f: note("G5"), sec: 1.9, env: adsr({ a: 0.6, d: 1.0, s: 0.5, r: 0.6 }), gain: 0.7 }),
    );
    const last = { decay: 0.3, release: 0.15 };
    return air(mix(2.5, [
      [0.0, whoosh({ sec: 0.62, from: 3400, to: 700, q: 0.8, seed: 81, env: adsr({ a: 0.38, d: 0.12, s: 0.2, r: 0.18 }) }), 0.35],
      [0.45, chime(note("G6"), 0.3, { decay: 0.08, shimmer: 0.4 }), 0.18],
      [0.58, pluck(note("C5"), 0.2, 0.07), 0.6],
      [0.72, chime(note("G5"), 0.3, { decay: 0.12 }), 0.5],
      [1.28, tone({ f: glide(note("C5"), note("G5"), 0.1), sec: 0.24, wave: "soft", env: adsr({ a: 0.01, d: 0.08, r: 0.06 }) }), 0.45],
      [0.6, pad, 0.1],
      ...tune.map(([at, n, sec]) => [at, chime(note(n), sec, { decay: sec * 0.7, shimmer: 0.5 }), 0.6]),
      [2.1, chime(note("G6"), 0.4, last), 0.6],
      [2.1, chime(note("E6"), 0.4, last), 0.3],
      [2.1, chime(note("C6"), 0.4, last), 0.3],
    ]), { wet: 0.2 });
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
