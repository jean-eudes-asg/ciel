// The website's sound: a calm bed under the sky, made in Web Audio with no
// files — slow air, a soft chord that breathes, a glassy chime now and then —
// that follows the scene on screen: bright by day, warm at golden hour and
// sunset, low and starry at night, out in space, wind and far thunder in a
// storm. Ciel's own sounds (core/sound, and the demo's) play on top of it.
// Off until the visitor turns it on; suspended while the tab is hidden.

export type AmbientScene = "clear" | "golden" | "cloudy" | "sunset" | "night" | "space" | "storm";

interface Mood {
  /** Four voices, as MIDI notes, low to high: they glide into the next scene's. */
  chord: [number, number, number, number];
  /** How bright the chord is: its low-pass cutoff, Hz. */
  bright: number;
  /** The air: how much, and where it sits (Hz). */
  wind: number;
  windHz: number;
  /** The chimes: notes to pick from (MIDI), and the average wait between two (s); none at 0. */
  bells: number[];
  every: number;
  /** Far thunder now and then. */
  thunder?: true;
}

const MOODS: Record<AmbientScene, Mood> = {
  // D, a lifted major ninth: open daylight.
  clear: { chord: [50, 57, 64, 66], bright: 1500, wind: 0.5, windHz: 700, bells: [74, 76, 78, 81, 83], every: 9 },
  // G major seventh: warm, waiting.
  golden: { chord: [43, 50, 59, 66], bright: 1250, wind: 0.45, windHz: 620, bells: [79, 81, 83, 86, 88], every: 8 },
  // A, suspended: grey and soft.
  cloudy: { chord: [45, 52, 59, 64], bright: 850, wind: 0.85, windHz: 460, bells: [81, 83, 88], every: 15 },
  // E minor ninth: the day letting go.
  sunset: { chord: [52, 59, 62, 66], bright: 1050, wind: 0.4, windHz: 540, bells: [76, 79, 81, 83, 86], every: 10 },
  // F major seventh, low: night air, stars twinkling high up.
  night: { chord: [41, 48, 52, 57], bright: 680, wind: 0.32, windHz: 380, bells: [81, 84, 88, 89, 91], every: 7 },
  // C lydian: weightless.
  space: { chord: [36, 43, 50, 54], bright: 600, wind: 0.16, windHz: 300, bells: [86, 88, 90, 93, 95], every: 6 },
  // D minor seventh under the wind, no chimes, thunder far away.
  storm: { chord: [38, 45, 53, 60], bright: 620, wind: 1.5, windHz: 330, bells: [], every: 0, thunder: true },
};

const hz = (midi: number) => 440 * 2 ** ((midi - 69) / 12);
const rand = (lo: number, hi: number) => lo + Math.random() * (hi - lo);

/** The whole bed's level: about 12 dB under Ciel's own sounds (at volume 0.2), peak to peak. */
const LEVEL = 1;
const FADE_S = 2.2;
/** How long a glide into a new scene takes (time constant, s). */
const GLIDE_S = 1.4;

export class Ambient {
  private ctx: AudioContext | null = null;
  private master: GainNode | null = null;
  private dry: GainNode | null = null;
  private wet: GainNode | null = null;
  private voices: OscillatorNode[][] = [];
  private padFilter: BiquadFilterNode | null = null;
  private windGain: GainNode | null = null;
  private windFilter: BiquadFilterNode | null = null;
  private noise: AudioBuffer | null = null;
  private timers = new Set<number>();
  private scene: AmbientScene = "clear";
  private on = false;
  private suspendTimer: number | null = null;

  get playing(): boolean {
    return this.on;
  }

  /** Call from a click: browsers only let sound start from one. */
  start() {
    if (this.on) return;
    this.on = true;
    if (!this.ctx) this.build();
    const ctx = this.ctx!;
    if (this.suspendTimer != null) window.clearTimeout(this.suspendTimer);
    this.suspendTimer = null;
    void ctx.resume();
    const g = this.master!.gain;
    g.cancelScheduledValues(ctx.currentTime);
    g.setValueAtTime(g.value, ctx.currentTime);
    g.linearRampToValueAtTime(LEVEL, ctx.currentTime + FADE_S);
    this.apply(true);
    this.loop(() => rand(3, 7), () => this.gust());
    this.loop(() => this.bellWait(), () => this.bell());
    this.loop(() => rand(9, 18), () => this.thunder());
  }

  stop() {
    if (!this.on || !this.ctx) return;
    this.on = false;
    for (const id of this.timers) window.clearTimeout(id);
    this.timers.clear();
    const ctx = this.ctx;
    const g = this.master!.gain;
    g.cancelScheduledValues(ctx.currentTime);
    g.setValueAtTime(g.value, ctx.currentTime);
    g.linearRampToValueAtTime(0, ctx.currentTime + 1.2);
    // Nothing left running once it is silent.
    this.suspendTimer = window.setTimeout(() => {
      this.suspendTimer = null;
      if (!this.on) void ctx.suspend();
    }, 1400);
  }

  /** The tab hidden: no sound and no work; shown again, it comes back. */
  pause(hidden: boolean) {
    if (!this.ctx || !this.on) return;
    void (hidden ? this.ctx.suspend() : this.ctx.resume());
  }

  setScene(scene: AmbientScene) {
    if (scene === this.scene) return;
    this.scene = scene;
    if (this.on) this.apply(false);
  }

  // ── The graph ───────────────────────────────────────────────────────────────

  private build() {
    const ctx = new AudioContext();
    this.ctx = ctx;
    this.master = ctx.createGain();
    this.master.gain.value = 0;
    this.master.connect(ctx.destination);

    this.dry = ctx.createGain();
    this.dry.connect(this.master);
    const reverb = ctx.createConvolver();
    reverb.buffer = this.impulse(4.2);
    this.wet = ctx.createGain();
    this.wet.gain.value = 0.9;
    this.wet.connect(reverb);
    reverb.connect(this.master);

    // The chord: four voices of two slightly detuned oscillators, each breathing on its own.
    this.padFilter = ctx.createBiquadFilter();
    this.padFilter.type = "lowpass";
    this.padFilter.Q.value = 0.4;
    const pad = ctx.createGain();
    pad.gain.value = 0.03;
    this.padFilter.connect(pad);
    pad.connect(this.dry);
    pad.connect(this.wet);
    const chord = MOODS[this.scene].chord;
    this.voices = chord.map((note, i) => {
      const voice = ctx.createGain();
      voice.gain.value = 0.22;
      const breath = ctx.createOscillator();
      breath.frequency.value = 0.045 + i * 0.017;
      const depth = ctx.createGain();
      depth.gain.value = 0.14;
      breath.connect(depth).connect(voice.gain);
      breath.start();
      const oscs = [0, 6].map((cents, k) => {
        const o = ctx.createOscillator();
        o.type = k === 0 ? "sine" : "triangle";
        o.frequency.value = hz(note);
        o.detune.value = cents;
        const level = ctx.createGain();
        level.gain.value = k === 0 ? 1 : 0.35;
        o.connect(level).connect(voice);
        o.start();
        return o;
      });
      voice.connect(this.padFilter!);
      return oscs;
    });

    // The air: noise, band-passed where the scene's wind sits.
    this.noise = this.noiseBuffer(6);
    const air = ctx.createBufferSource();
    air.buffer = this.noise;
    air.loop = true;
    this.windFilter = ctx.createBiquadFilter();
    this.windFilter.type = "bandpass";
    this.windFilter.Q.value = 0.55;
    this.windGain = ctx.createGain();
    this.windGain.gain.value = 0;
    air.connect(this.windFilter).connect(this.windGain);
    this.windGain.connect(this.dry);
    this.windGain.connect(this.wet);
    air.start();

    document.addEventListener("visibilitychange", () => this.pause(document.hidden));
  }

  /** Glides the chord, its brightness and the air into the current scene's. */
  private apply(now: boolean) {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const m = MOODS[this.scene];
    const tc = now ? 0.05 : GLIDE_S;
    m.chord.forEach((note, i) => {
      for (const o of this.voices[i]) o.frequency.setTargetAtTime(hz(note), t, tc);
    });
    this.padFilter!.frequency.setTargetAtTime(m.bright, t, tc * 1.3);
    this.windFilter!.frequency.setTargetAtTime(m.windHz, t, tc * 1.3);
    this.windGain!.gain.setTargetAtTime(this.windLevel(1), t, tc * 1.3);
  }

  private windLevel(gust: number): number {
    return 0.05 * MOODS[this.scene].wind * gust;
  }

  /** The wind rises and falls, never twice the same. */
  private gust() {
    const ctx = this.ctx!;
    const t = ctx.currentTime;
    const m = MOODS[this.scene];
    this.windGain!.gain.setTargetAtTime(this.windLevel(rand(0.55, 1.35)), t, rand(1.2, 2.6));
    this.windFilter!.frequency.setTargetAtTime(m.windHz * rand(0.75, 1.3), t, rand(1.5, 3));
  }

  private bellWait(): number {
    const every = MOODS[this.scene].every;
    return every > 0 ? rand(every * 0.5, every * 1.5) : 4;
  }

  /** A glass chime: a few inharmonic partials, a soft strike, a long tail into the reverb. */
  private bell() {
    const m = MOODS[this.scene];
    if (!m.bells.length) return;
    const ctx = this.ctx!;
    const t = ctx.currentTime + 0.02;
    const f = hz(m.bells[Math.floor(Math.random() * m.bells.length)]);
    const night = this.scene === "night" || this.scene === "space";
    const peak = (night ? 0.022 : 0.03) * rand(0.6, 1);
    const pan = ctx.createStereoPanner();
    pan.pan.value = rand(-0.65, 0.65);
    const out = ctx.createGain();
    out.gain.value = 1;
    out.connect(pan);
    const send = ctx.createGain();
    send.gain.value = 0.35;
    pan.connect(send).connect(this.dry!);
    pan.connect(this.wet!);
    const partials: [number, number, number][] = [
      [1, 1, 3.6],
      [2.76, 0.22, 1.6],
      [5.4, 0.07, 0.8],
    ];
    for (const [ratio, amp, decay] of partials) {
      const o = ctx.createOscillator();
      o.frequency.value = f * ratio;
      const g = ctx.createGain();
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(peak * amp, t + 0.012);
      g.gain.exponentialRampToValueAtTime(0.0001, t + decay);
      o.connect(g).connect(out);
      o.start(t);
      o.stop(t + decay + 0.1);
    }
  }

  /** Thunder far away: low noise, a slow swell, a long roll. */
  private thunder() {
    if (!MOODS[this.scene].thunder) return;
    const ctx = this.ctx!;
    const t = ctx.currentTime + 0.05;
    const src = ctx.createBufferSource();
    src.buffer = this.noise;
    const low = ctx.createBiquadFilter();
    low.type = "lowpass";
    low.frequency.value = rand(90, 150);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(rand(0.14, 0.24), t + rand(0.5, 1.1));
    g.gain.exponentialRampToValueAtTime(0.0001, t + rand(3.5, 5.5));
    src.connect(low).connect(g);
    g.connect(this.dry!);
    g.connect(this.wet!);
    src.start(t, rand(0, 4));
    src.stop(t + 6);
  }

  /** Calls `fn` again and again, `wait()` seconds apart, while the sound is on. */
  private loop(wait: () => number, fn: () => void) {
    const next = () => {
      const id = window.setTimeout(() => {
        this.timers.delete(id);
        if (!this.on) return;
        fn();
        next();
      }, wait() * 1000);
      this.timers.add(id);
    };
    next();
  }

  private noiseBuffer(seconds: number): AudioBuffer {
    const ctx = this.ctx!;
    const buf = ctx.createBuffer(2, Math.round(ctx.sampleRate * seconds), ctx.sampleRate);
    for (let c = 0; c < 2; c++) {
      const d = buf.getChannelData(c);
      // Brownish noise: softer than white, like air rather than hiss.
      let last = 0;
      for (let i = 0; i < d.length; i++) {
        last = (last + 0.02 * (Math.random() * 2 - 1)) / 1.02;
        d[i] = last * 3.5;
      }
    }
    return buf;
  }

  /** A hall's tail: decaying noise, a little different in each ear. */
  private impulse(seconds: number): AudioBuffer {
    const ctx = this.ctx!;
    const n = Math.round(ctx.sampleRate * seconds);
    const buf = ctx.createBuffer(2, n, ctx.sampleRate);
    for (let c = 0; c < 2; c++) {
      const d = buf.getChannelData(c);
      for (let i = 0; i < n; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / n) ** 3.2;
    }
    return buf;
  }
}
