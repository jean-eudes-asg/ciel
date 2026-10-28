// The games' own sounds, made here in code like the rest of Ciel's: a piece
// set down on wood, a capture, a check's bell, steps in the maze, a star
// picked up, the countdown, a little tune at the end, and the sky's wind (with
// rain and thunder in the storm). They follow Ciel's sound switch and volume
// (core/sound.ts); the context sleeps when nothing plays.

import { Sound } from "../core/sound";

let ctx: AudioContext | null = null;
let out: GainNode | null = null;
let noise: AudioBuffer | null = null;
let idle: number | null = null;
let bed: { stop(): void } | null = null;

function audio(): { c: AudioContext; o: GainNode } | null {
  if (!Sound.enabled || Sound.volume <= 0) return null;
  if (!ctx) {
    const Ctor = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctor) return null;
    ctx = new Ctor();
    out = ctx.createGain();
    out.connect(ctx.destination);
    noise = ctx.createBuffer(1, ctx.sampleRate * 2, ctx.sampleRate);
    const d = noise.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }
  // Ciel's sounds play at 0..0.2; these are made a little louder to sit beside them.
  out!.gain.value = Sound.volume * 4;
  if (ctx.state === "suspended") void ctx.resume();
  if (idle !== null) window.clearTimeout(idle);
  idle = window.setTimeout(() => {
    idle = null;
    if (!bed) void ctx?.suspend();
  }, 2500);
  return { c: ctx, o: out! };
}

function env(c: AudioContext, at: number, peak: number, attack: number, decay: number): GainNode {
  const g = c.createGain();
  g.gain.setValueAtTime(0, at);
  g.gain.linearRampToValueAtTime(peak, at + attack);
  g.gain.exponentialRampToValueAtTime(0.0001, at + attack + decay);
  return g;
}

function tone(freq: number, at: number, peak: number, decay: number, type: OscillatorType = "sine", glideTo?: number) {
  const a = audio();
  if (!a) return;
  const o = a.c.createOscillator();
  o.type = type;
  o.frequency.setValueAtTime(freq, at);
  if (glideTo) o.frequency.exponentialRampToValueAtTime(glideTo, at + decay);
  const g = env(a.c, at, peak, 0.004, decay);
  o.connect(g).connect(a.o);
  o.start(at);
  o.stop(at + decay + 0.05);
}

function hiss(at: number, peak: number, decay: number, filter: BiquadFilterType, freq: number, q = 1) {
  const a = audio();
  if (!a || !noise) return;
  const s = a.c.createBufferSource();
  s.buffer = noise;
  s.playbackRate.value = 0.8 + Math.random() * 0.4;
  const f = a.c.createBiquadFilter();
  f.type = filter;
  f.frequency.value = freq;
  f.Q.value = q;
  const g = env(a.c, at, peak, 0.003, decay);
  s.connect(f).connect(g).connect(a.o);
  s.start(at, Math.random());
  s.stop(at + decay + 0.05);
}

const now = () => audio()?.c.currentTime ?? 0;

/** A piece set down on the board. */
function tock(at: number, weight = 1) {
  const pitch = 0.9 + Math.random() * 0.2;
  hiss(at, 0.5 * weight, 0.05, "bandpass", 1900 * pitch, 1.4);
  tone(230 * pitch, at, 0.55 * weight, 0.09, "sine", 110);
}

export const Sfx = {
  move() {
    tock(now());
  },
  capture() {
    const t = now();
    tock(t, 1.3);
    hiss(t + 0.03, 0.35, 0.12, "lowpass", 900);
  },
  castle() {
    const t = now();
    tock(t);
    tock(t + 0.12, 0.8);
  },
  check() {
    const t = now();
    tone(988, t, 0.16, 0.9, "sine");
    tone(1482, t + 0.01, 0.08, 0.7, "sine");
  },
  pick() {
    tone(1300, now(), 0.05, 0.05, "triangle");
  },
  step() {
    hiss(now(), 0.12, 0.05, "lowpass", 700 + Math.random() * 400);
  },
  bump() {
    const t = now();
    tone(120, t, 0.4, 0.12, "sine", 60);
    hiss(t, 0.15, 0.08, "lowpass", 400);
  },
  star() {
    const t = now();
    [1319, 1760, 2637].forEach((f, k) => tone(f, t + k * 0.06, 0.1, 0.35, "triangle"));
  },
  count(go: boolean) {
    tone(go ? 988 : 660, now(), 0.14, go ? 0.5 : 0.18, "triangle");
  },
  /** A short tune: up when you win, down when the rival does, level for a draw. */
  tune(kind: "win" | "lose" | "draw" | "start") {
    const t = now();
    const notes = { win: [523, 659, 784, 1047], lose: [659, 523, 440, 392], draw: [587, 587, 698], start: [392, 523, 659] }[kind];
    notes.forEach((f, k) => {
      tone(f, t + k * 0.13, 0.12, 0.6, "triangle");
      tone(f * 2, t + k * 0.13, 0.025, 0.4, "sine");
    });
  },
  thunder(far: number) {
    const t = now() + far;
    hiss(t, 0.7, 2.6, "lowpass", 160, 0.7);
    hiss(t + 0.15, 0.4, 1.6, "lowpass", 320, 0.7);
  },

  /** The sky's wind under the maze, with rain in the storm; one at a time. */
  ambience(rain: boolean) {
    Sfx.quiet();
    const a = audio();
    if (!a || !noise) return;
    const { c, o } = a;
    const parts: AudioScheduledSourceNode[] = [];
    const layer = (filter: BiquadFilterType, freq: number, level: number, sway: number) => {
      const s = c.createBufferSource();
      s.buffer = noise;
      s.loop = true;
      const f = c.createBiquadFilter();
      f.type = filter;
      f.frequency.value = freq;
      f.Q.value = 0.8;
      const g = c.createGain();
      g.gain.value = 0;
      g.gain.linearRampToValueAtTime(level, c.currentTime + 2);
      // The wind rises and falls.
      const lfo = c.createOscillator();
      lfo.frequency.value = 0.07 + Math.random() * 0.05;
      const depth = c.createGain();
      depth.gain.value = sway;
      lfo.connect(depth).connect(f.frequency);
      s.connect(f).connect(g).connect(o);
      s.start();
      lfo.start();
      parts.push(s, lfo);
    };
    layer("bandpass", 420, 0.05, 220);
    if (rain) layer("highpass", 2600, 0.035, 300);
    bed = {
      stop: () => {
        for (const p of parts) p.stop();
      },
    };
  },
  quiet() {
    bed?.stop();
    bed = null;
  },
};
