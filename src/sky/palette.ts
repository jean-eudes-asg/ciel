// The light of a scene. The sky's colours follow the sun's elevation through
// keyframes inspired by real skies (Rayleigh blue overhead, a pale horizon at
// noon, gold then orange and pink as the light crosses more air, the violet
// and blue hours after sunset, a navy night), and each theme bends them: grey
// for an overcast day, slate for a storm, a warm horizon for a sunset that
// happens at night.

import type { Theme } from "./prefs";

export type RGB = readonly [number, number, number];

export function hex(h: string): RGB {
  const v = parseInt(h.replace("#", ""), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

export const mix = (a: RGB, b: RGB, t: number): RGB => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

export const scale = (c: RGB, k: number): RGB => [c[0] * k, c[1] * k, c[2] * k];

export function css(c: RGB, alpha = 1): string {
  const r = Math.round(Math.min(255, Math.max(0, c[0])));
  const g = Math.round(Math.min(255, Math.max(0, c[1])));
  const b = Math.round(Math.min(255, Math.max(0, c[2])));
  return `rgba(${r},${g},${b},${Math.min(1, Math.max(0, alpha))})`;
}

export const luma = (c: RGB) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];

/** Halfway to grey of the same brightness, by `amount`. */
export const desaturate = (c: RGB, amount: number): RGB => {
  const l = luma(c);
  return mix(c, [l, l, l], amount);
};

// ── The sky by sun elevation ──────────────────────────────────────────────────

interface Key {
  e: number;
  zenith: string;
  upper: string;
  mid: string;
  horizon: string;
  glow: string;
  glowAmt: number;
  sun: string;
  /** Clouds: the side facing the light, the middle, the side away from it. */
  lit: string;
  cmid: string;
  shade: string;
}

const KEYS: Key[] = [
  { e: -24, zenith: "#02040f", upper: "#050a1d", mid: "#09112b", horizon: "#121c3e", glow: "#1a2450", glowAmt: 0, sun: "#ff6a3c",
    lit: "#46557e", cmid: "#222c50", shade: "#0d142e" },
  { e: -14, zenith: "#050a22", upper: "#0a1333", mid: "#131d45", horizon: "#25305f", glow: "#33356f", glowAmt: 0.12, sun: "#ff6a3c",
    lit: "#56628e", cmid: "#2a3360", shade: "#121a3a" },
  { e: -9, zenith: "#0b1440", upper: "#162257", mid: "#283272", horizon: "#544883", glow: "#7f5592", glowAmt: 0.32, sun: "#ff6a3c",
    lit: "#7c6a9c", cmid: "#3f3f74", shade: "#1a2048" },
  { e: -6, zenith: "#142152", upper: "#26336d", mid: "#46468a", horizon: "#a35f88", glow: "#d86c88", glowAmt: 0.48, sun: "#ff6a3c",
    lit: "#c0789a", cmid: "#6a5888", shade: "#272b5a" },
  { e: -3, zenith: "#1e2f68", upper: "#384885", mid: "#755f9c", horizon: "#e9786c", glow: "#ff7d62", glowAmt: 0.68, sun: "#ff6a3c",
    lit: "#ff9a86", cmid: "#c27896", shade: "#3c3c70" },
  { e: 0, zenith: "#28407c", upper: "#4b66a2", mid: "#ab7896", horizon: "#ff9050", glow: "#ff7e3e", glowAmt: 0.9, sun: "#ff8a46",
    lit: "#ffb07a", cmid: "#e5888a", shade: "#5a5286" },
  { e: 2.5, zenith: "#30528c", upper: "#5a82b8", mid: "#c49a9e", horizon: "#ffb267", glow: "#ff9c4a", glowAmt: 0.85, sun: "#ffb35c",
    lit: "#ffd4a2", cmid: "#eeb19c", shade: "#857aa6" },
  { e: 6, zenith: "#2c5b9c", upper: "#5689c4", mid: "#a2b4cf", horizon: "#ffd29a", glow: "#ffc274", glowAmt: 0.68, sun: "#ffe0a0",
    lit: "#fff1da", cmid: "#f2d8c6", shade: "#9a96b8" },
  { e: 12, zenith: "#2861ab", upper: "#4a8dd0", mid: "#8cb9e2", horizon: "#d8e6ef", glow: "#fff0d0", glowAmt: 0.45, sun: "#fff6dc",
    lit: "#ffffff", cmid: "#eef2f8", shade: "#a8b9d2" },
  { e: 25, zenith: "#1d5bb0", upper: "#3a85d5", mid: "#77b0e5", horizon: "#cde2f3", glow: "#fff8e8", glowAmt: 0.36, sun: "#fffdf4",
    lit: "#ffffff", cmid: "#f0f5fb", shade: "#a6bbd8" },
  { e: 45, zenith: "#1755b2", upper: "#3180d7", mid: "#6ca8e5", horizon: "#c4ddf3", glow: "#ffffff", glowAmt: 0.3, sun: "#ffffff",
    lit: "#ffffff", cmid: "#f1f6fc", shade: "#a3badb" },
  { e: 90, zenith: "#1250b2", upper: "#2d7cd7", mid: "#64a3e5", horizon: "#bed9f2", glow: "#ffffff", glowAmt: 0.28, sun: "#ffffff",
    lit: "#ffffff", cmid: "#f1f6fc", shade: "#a3badb" },
];

type Sample = { [K in Exclude<keyof Key, "e" | "glowAmt">]: RGB } & { glowAmt: number };

export function skyAt(elevation: number): Sample {
  const e = Math.min(90, Math.max(-24, elevation));
  let i = 0;
  while (i < KEYS.length - 2 && KEYS[i + 1].e < e) i++;
  const a = KEYS[i];
  const b = KEYS[i + 1];
  const t = Math.min(1, Math.max(0, (e - a.e) / (b.e - a.e)));
  const m = (k: Exclude<keyof Key, "e" | "glowAmt">) => mix(hex(a[k]), hex(b[k]), t);
  return {
    zenith: m("zenith"), upper: m("upper"), mid: m("mid"), horizon: m("horizon"),
    glow: m("glow"), sun: m("sun"), lit: m("lit"), cmid: m("cmid"), shade: m("shade"),
    glowAmt: a.glowAmt + (b.glowAmt - a.glowAmt) * t,
  };
}

// ── A theme's light ───────────────────────────────────────────────────────────

export interface Light {
  zenith: RGB;
  upper: RGB;
  mid: RGB;
  horizon: RGB;
  glow: RGB;
  /** How strongly the sun's glow colours the sky (0…1). */
  glowAmt: number;
  sunColor: RGB;
  /** Stars and the Milky Way, 0…1. */
  stars: number;
  milkyWay: number;
  lit: RGB;
  cmid: RGB;
  shade: RGB;
  /** Cumulus drifting in the sky, and how big. */
  cumulus: number;
  cumulusSize: number;
  /** The high cloud deck: coverage (0…1) and how thick it looks. */
  deck: number;
  deckThick: number;
  rain: number;
  lightning: boolean;
  /** The pink band opposite the sun just after sunset. */
  belt: number;
  /** A warm horizon under a night sky (a sunset theme after dark). */
  afterglow: number;
  /** Ciel's shade side takes this colour; his lit side the rim. */
  ambient: RGB;
  ambientAmt: number;
  rim: RGB;
  rimAmt: number;
  /** Overall darkness of things in the scene (0 = full day). */
  dark: number;
}

const STORM: RGB = hex("#2a333e");

/**
 * The light for a theme, with the sun drawn at `elevation`. `afterglow`: a
 * sunset or golden theme after dark.
 */
export function lightFor(theme: Theme, elevation: number, afterglow: boolean): Light {
  const s = skyAt(elevation);
  const night = Math.min(1, Math.max(0, (-elevation - 4) / 12));
  const l: Light = {
    zenith: s.zenith, upper: s.upper, mid: s.mid, horizon: s.horizon,
    glow: s.glow, glowAmt: s.glowAmt, sunColor: s.sun,
    stars: Math.min(1, Math.max(0, (-elevation - 5) / 9)),
    milkyWay: Math.min(1, Math.max(0, (-elevation - 13) / 6)),
    lit: s.lit, cmid: s.cmid, shade: s.shade,
    cumulus: 7, cumulusSize: 1, deck: 0.12, deckThick: 0.3,
    rain: 0, lightning: false,
    belt: elevation < 2 && elevation > -7 ? 1 - Math.abs(elevation + 2) / 5 : 0,
    afterglow: 0,
    ambient: mix(s.shade, s.zenith, 0.4), ambientAmt: 0.18 + night * 0.4,
    rim: s.sun, rimAmt: elevation > 15 ? 0.12 : elevation > -4 ? 0.45 : 0.15,
    dark: night,
  };
  switch (theme) {
    case "clear":
      l.cumulus = 7;
      l.deck = 0.08;
      break;
    case "golden":
      l.cumulus = 6;
      l.deck = 0.3;
      l.deckThick = 0.25;
      break;
    case "sunset":
      l.cumulus = 5;
      l.deck = 0.5;
      l.deckThick = 0.3;
      break;
    case "night":
      l.cumulus = 4;
      l.deck = 0.18;
      l.ambientAmt = 0.55;
      l.rim = hex("#c9d6ff");
      l.rimAmt = 0.25;
      break;
    case "cloudy": {
      const k = 0.72;
      l.zenith = mix(desaturate(s.zenith, k), hex("#9aa6b4"), 0.35 * (1 - night));
      l.upper = mix(desaturate(s.upper, k), hex("#aeb8c4"), 0.35 * (1 - night));
      l.mid = desaturate(s.mid, k * 0.85);
      l.horizon = desaturate(s.horizon, 0.45);
      l.glowAmt *= 0.55;
      l.lit = mix(desaturate(s.lit, 0.5), hex("#e1e5eb"), 0.5 * (1 - night));
      l.cmid = mix(desaturate(s.cmid, 0.6), hex("#a9b2bd"), 0.6 * (1 - night));
      l.shade = mix(desaturate(s.shade, 0.6), hex("#66717f"), 0.6 * (1 - night));
      l.cumulus = 9;
      l.cumulusSize = 1.25;
      l.deck = 0.86;
      l.deckThick = 0.8;
      l.stars *= 0.15;
      l.milkyWay = 0;
      l.belt *= 0.3;
      l.rimAmt *= 0.4;
      l.ambient = l.shade;
      l.ambientAmt = 0.22 + night * 0.35;
      break;
    }
    case "storm": {
      const dim = 0.55 + 0.45 * (1 - night);
      l.zenith = scale(mix(s.zenith, STORM, 0.82), dim * 0.85);
      l.upper = scale(mix(s.upper, hex("#36414c"), 0.8), dim * 0.9);
      l.mid = scale(mix(s.mid, hex("#4a5660"), 0.78), dim);
      l.horizon = scale(mix(s.horizon, hex("#6d7a78"), 0.7), dim);
      l.glow = mix(s.glow, hex("#c9b48a"), 0.5);
      l.glowAmt *= 0.3;
      l.lit = scale(mix(s.lit, hex("#8c96a2"), 0.8), dim);
      l.cmid = scale(mix(s.cmid, hex("#4d5764"), 0.85), dim);
      l.shade = scale(mix(s.shade, hex("#1a2028"), 0.9), dim);
      l.cumulus = 6;
      l.cumulusSize = 1.45;
      l.deck = 1;
      l.deckThick = 1;
      l.rain = 1;
      l.lightning = true;
      l.stars = 0;
      l.milkyWay = 0;
      l.belt = 0;
      l.rimAmt = 0.08;
      l.ambient = l.shade;
      l.ambientAmt = 0.4;
      l.dark = Math.max(0.45, night);
      break;
    }
    case "space":
      l.stars = 1;
      l.cumulus = 0;
      l.deck = 0;
      l.belt = 0;
      l.ambient = hex("#1a1440");
      l.ambientAmt = 0.4;
      l.rim = hex("#9fd8ff");
      l.rimAmt = 0.35;
      l.dark = 1;
      break;
  }
  if (afterglow) {
    // The day is over but the mood is a sunset: a navy sky warming to amber
    // and rose along the horizon.
    l.afterglow = theme === "sunset" ? 1 : 0.75;
    l.horizon = mix(l.horizon, hex("#c46a68"), 0.55 * l.afterglow);
    l.mid = mix(l.mid, hex("#3a2f62"), 0.4);
    l.glow = hex("#ff8a5a");
    l.glowAmt = 0.5 * l.afterglow;
    l.lit = mix(l.lit, hex("#c98a8a"), 0.45);
    l.cmid = mix(l.cmid, hex("#5a4470"), 0.3);
    l.stars *= 0.75;
    l.milkyWay *= 0.4;
    l.rim = hex("#ffb08a");
    l.rimAmt = 0.3;
  }
  return l;
}
