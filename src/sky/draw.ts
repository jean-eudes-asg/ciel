// The painters of a sky, each one layer: the gradient and the sun's glow,
// stars and the Milky Way, the moon, the sun, the high cloud deck seen in
// perspective, the drifting cumulus, the sea of clouds below, rain and
// lightning, and deep space with its nebula and a planet. Everything is
// sized from the canvas height, so a thumbnail and a 3440×1440 screen show
// the same picture; everything random comes from the scene's seed.

import { Noise, clamp01, hash01, rng, smoothstep } from "./noise";
import { css, hex, mix, type Light, type RGB } from "./palette";
import type { SceneSpec } from "./scenes";
import { makeCanvas, context, type AnyCanvas } from "./canvas";

type Ctx = CanvasRenderingContext2D;

/** Where the sky meets the sea of clouds, as a fraction of the height. */
export const HORIZON = 0.8;
const RAD = Math.PI / 180;

export interface Pt {
  x: number;
  y: number;
}

/**
 * Sky → screen (fractions of the width and height). The view faces the
 * equator; azimuths are squeezed so the sun crosses the whole screen in a
 * day, rising on one side and setting on the other.
 */
export function toScreen(elevation: number, azimuth: number, facing: "south" | "north"): Pt {
  const d = ((((azimuth - (facing === "south" ? 180 : 0)) % 360) + 540) % 360) - 180;
  const x = 0.5 + (0.45 * Math.tanh(d / 75)) / Math.tanh(180 / 75);
  const y = elevation >= 0
    ? HORIZON - (HORIZON - 0.07) * Math.pow(Math.sin(Math.min(90, elevation) * RAD), 0.75)
    : HORIZON + Math.min(0.6, -elevation * 0.012);
  return { x, y };
}

/** Everything the painters share for one scene at one size. */
export interface Frame {
  W: number;
  H: number;
  spec: SceneSpec;
  light: Light;
  sun: Pt;
  moon: Pt | null;
  /** The main light: the sun, else the moon, else straight above. In pixels. */
  lightAt: Pt;
  /** Light strength for highlights (night is dim). */
  lightAmt: number;
  noise: Noise;
  /** One pixel at 1440 px high: line widths and star sizes follow it. */
  px: number;
}

export function frameFor(spec: SceneSpec, light: Light, W: number, H: number): Frame {
  const sun = toScreen(spec.sun.elevation, spec.sun.azimuth, spec.facing);
  const moon = spec.moon ? toScreen(spec.moon.elevation, spec.moon.azimuth, spec.facing) : null;
  let lightAt: Pt = { x: sun.x * W, y: sun.y * H };
  let lightAmt = 1;
  if (spec.sun.elevation < -7 && !spec.afterglow) {
    if (moon) {
      lightAt = { x: moon.x * W, y: moon.y * H };
      lightAmt = 0.55 * Math.sin(Math.PI * spec.moon!.phase) + 0.15;
    } else {
      lightAt = { x: W * 0.5, y: -H * 2 };
      lightAmt = 0.25;
    }
  }
  if (spec.theme === "space") {
    lightAt = { x: W * 0.15, y: -H * 0.2 };
    lightAmt = 0.6;
  }
  // Under an overcast the light is diffuse: soft, grey shading.
  if (spec.theme === "cloudy") lightAmt *= 0.72;
  if (spec.theme === "storm") lightAmt *= 0.5;
  return {
    W, H, spec, light, sun, moon, lightAt, lightAmt,
    noise: new Noise(spec.seed),
    px: Math.max(0.5, H / 1440),
  };
}

// ── The sky ───────────────────────────────────────────────────────────────────

export function paintSky(c: Ctx, f: Frame) {
  const { W, H, light: l } = f;
  const g = c.createLinearGradient(0, 0, 0, H);
  g.addColorStop(0, css(l.zenith));
  g.addColorStop(0.3, css(l.upper));
  g.addColorStop(0.6, css(l.mid));
  g.addColorStop(HORIZON, css(l.horizon));
  g.addColorStop(1, css(mix(l.horizon, l.mid, 0.4)));
  c.fillStyle = g;
  c.fillRect(0, 0, W, H);

  const hy = HORIZON * H;
  // The belt of Venus: pink above the earth's blue shadow, opposite the sun.
  if (l.belt > 0.02) {
    const b = c.createLinearGradient(0, hy - 0.24 * H, 0, hy);
    b.addColorStop(0, css(hex("#e8a3b8"), 0));
    b.addColorStop(0.5, css(hex("#ee9fb4"), 0.26 * l.belt));
    b.addColorStop(0.78, css(hex("#7a70a8"), 0.22 * l.belt));
    b.addColorStop(1, css(hex("#4c5690"), 0.34 * l.belt));
    c.fillStyle = b;
    c.fillRect(0, hy - 0.24 * H, W, 0.24 * H);
  }
  // A warm horizon under the night (a sunset theme after dark).
  if (l.afterglow > 0) {
    const a = c.createLinearGradient(0, hy - 0.32 * H, 0, hy);
    a.addColorStop(0, css(hex("#ff8a5a"), 0));
    a.addColorStop(0.6, css(hex("#c25a78"), 0.16 * l.afterglow));
    a.addColorStop(1, css(hex("#ff9a62"), 0.5 * l.afterglow));
    c.fillStyle = a;
    c.fillRect(0, hy - 0.32 * H, W, 0.32 * H);
  }
  paintGlow(c, f);
  if (f.spec.theme !== "space") paintHorizonHaze(c, f);
}

/** The sun's glow: wide over the sky, and a bright band along the horizon. */
function paintGlow(c: Ctx, f: Frame) {
  const { W, H, light: l } = f;
  if (l.glowAmt <= 0.01 || f.spec.theme === "space") return;
  const sx = f.sun.x * W;
  const sy = Math.min(1.35, f.sun.y) * H;
  const r = Math.max(W, H) * 0.95;
  const g = c.createRadialGradient(sx, sy, 0, sx, sy, r);
  g.addColorStop(0, css(l.glow, 0.75 * l.glowAmt));
  g.addColorStop(0.1, css(l.glow, 0.5 * l.glowAmt));
  g.addColorStop(0.35, css(l.glow, 0.2 * l.glowAmt));
  g.addColorStop(0.7, css(l.glow, 0.05 * l.glowAmt));
  g.addColorStop(1, css(l.glow, 0));
  c.fillStyle = g;
  c.fillRect(0, 0, W, H);

  c.save();
  c.translate(sx, HORIZON * H);
  c.scale(1, 0.2);
  const b = c.createRadialGradient(0, 0, 0, 0, 0, W * 0.75);
  b.addColorStop(0, css(mix(l.glow, l.sunColor, 0.4), 0.85 * l.glowAmt));
  b.addColorStop(0.3, css(l.glow, 0.4 * l.glowAmt));
  b.addColorStop(1, css(l.glow, 0));
  c.fillStyle = b;
  c.fillRect(-W * 2, -H * 4, W * 4, H * 8);
  c.restore();
}

/** A thin haze where the sky meets the clouds below. */
function paintHorizonHaze(c: Ctx, f: Frame) {
  const { W, H, light: l } = f;
  const hy = HORIZON * H;
  const g = c.createLinearGradient(0, hy - 0.12 * H, 0, hy + 0.02 * H);
  g.addColorStop(0, css(l.horizon, 0));
  g.addColorStop(0.8, css(l.horizon, 0.35));
  g.addColorStop(1, css(l.horizon, 0.5));
  c.fillStyle = g;
  c.fillRect(0, hy - 0.12 * H, W, 0.14 * H);
}

/** A whisper of noise over smooth gradients, so a large sky never bands. */
export function paintDither(c: Ctx, f: Frame) {
  const tile = makeCanvas(64, 64);
  const t = context(tile);
  const img = t.createImageData(64, 64);
  const r = rng(f.spec.seed ^ 0x5eed);
  for (let i = 0; i < img.data.length; i += 4) {
    const v = r() < 0.5 ? 0 : 255;
    img.data[i] = v;
    img.data[i + 1] = v;
    img.data[i + 2] = v;
    img.data[i + 3] = Math.floor(r() * 7);
  }
  t.putImageData(img, 0, 0);
  const pattern = c.createPattern(tile as unknown as CanvasImageSource, "repeat");
  if (!pattern) return;
  c.fillStyle = pattern;
  c.fillRect(0, 0, f.W, f.H);
}

// ── Stars and the Milky Way ───────────────────────────────────────────────────

export interface Star {
  x: number;
  y: number;
  r: number;
  a: number;
  color: RGB;
  /** Twinkle speed and phase. */
  w: number;
  p: number;
}

const STAR_COLORS: RGB[] = ["#9fb4ff", "#c8d6ff", "#f4f6ff", "#fff6ea", "#ffe2bd", "#ffc49a"].map(hex);

function starColor(r: () => number): RGB {
  const v = r();
  if (v < 0.12) return STAR_COLORS[0];
  if (v < 0.35) return STAR_COLORS[1];
  if (v < 0.7) return STAR_COLORS[2];
  if (v < 0.86) return STAR_COLORS[3];
  if (v < 0.95) return STAR_COLORS[4];
  return STAR_COLORS[5];
}

/**
 * Paints the stars of the sky (or of space) and returns the brightest, which
 * the animation twinkles; they are not painted here.
 */
export function paintStars(c: Ctx, f: Frame, density: number, bottom: number): Star[] {
  const { W, H, light: l, px } = f;
  const visibility = l.stars;
  if (visibility <= 0.01) return [];
  const r = rng(f.spec.seed ^ 0x51a7);
  const count = Math.round(1500 * density * (W / H) / 1.78);
  const twinklers: Star[] = [];
  const fadeTop = bottom * H * 0.55;
  for (let i = 0; i < count; i++) {
    const x = r() * W;
    const y = Math.pow(r(), 1.15) * bottom * H;
    const b = Math.pow(r(), 5.5);
    const color = starColor(r);
    const extinction = bottom < 1 ? smoothstep(bottom * H, fadeTop, y) : 1;
    const a = (0.22 + 0.78 * b) * visibility * (0.35 + 0.65 * extinction);
    const size = (0.45 + b * 1.9) * px;
    const star: Star = { x, y, r: size, a, color, w: 0.6 + r() * 2.4, p: r() * Math.PI * 2 };
    if (b > 0.3 && twinklers.length < 160) {
      twinklers.push(star);
      continue;
    }
    if (a < 0.02) continue;
    c.fillStyle = css(color, a);
    if (size < 1.1) c.fillRect(x, y, Math.max(0.6, size * 1.4), Math.max(0.6, size * 1.4));
    else {
      c.beginPath();
      c.arc(x, y, size, 0, Math.PI * 2);
      c.fill();
    }
  }
  return twinklers;
}

/** One soft glow per star colour, drawn once; twinkling only copies them. */
const glowSprites = new Map<RGB, AnyCanvas>();
function glowSprite(color: RGB): AnyCanvas {
  let s = glowSprites.get(color);
  if (!s) {
    s = makeCanvas(32, 32);
    const c = context(s);
    const g = c.createRadialGradient(16, 16, 0, 16, 16, 16);
    g.addColorStop(0, css(mix(color, [255, 255, 255], 0.6), 1));
    g.addColorStop(0.25, css(color, 0.55));
    g.addColorStop(1, css(color, 0));
    c.fillStyle = g;
    c.fillRect(0, 0, 32, 32);
    glowSprites.set(color, s);
  }
  return s;
}

/** The twinkling stars, for time `t` (seconds); `still` shows them steady. */
export function paintTwinkle(c: Ctx, stars: Star[], t: number, still: boolean) {
  c.save();
  for (const s of stars) {
    const k = still ? 0.85 : 0.6 + 0.4 * Math.sin(t * s.w + s.p) * Math.sin(t * s.w * 0.37 + s.p * 2);
    const a = s.a * k;
    if (a < 0.02) continue;
    c.globalAlpha = Math.min(1, a);
    const r = s.r * 3.2;
    c.drawImage(glowSprite(s.color) as unknown as CanvasImageSource, s.x - r, s.y - r, r * 2, r * 2);
  }
  c.restore();
}

/** The Milky Way: a soft band of light with dark dust lanes, across the night. */
export function paintMilkyWay(c: Ctx, f: Frame) {
  const { W, H, light: l } = f;
  if (l.milkyWay <= 0.02) return;
  const r = rng(f.spec.seed ^ 0x3a11);
  // The band runs from low on one side to high on the other.
  const ax = (0.05 + r() * 0.3) * W;
  const ay = HORIZON * H;
  const bx = (0.6 + r() * 0.35) * W;
  const by = -0.1 * H;
  const dx = bx - ax;
  const dy = by - ay;
  const len = Math.hypot(dx, dy);
  const nx = -dy / len;
  const ny = dx / len;
  const lw = Math.max(80, Math.min(640, Math.round(W / 5)));
  const lh = Math.max(40, Math.round((lw * H * HORIZON) / W));
  const img = new ImageData(lw, lh);
  const n = f.noise;
  const width = 0.16 * H;
  for (let j = 0; j < lh; j++) {
    const y = ((j + 0.5) / lh) * HORIZON * H;
    for (let i = 0; i < lw; i++) {
      const x = ((i + 0.5) / lw) * W;
      const along = ((x - ax) * dx + (y - ay) * dy) / len;
      const across = (x - ax) * nx + (y - ay) * ny;
      const u = along / H;
      const v = across / H;
      const wob = n.fbm(u * 2.2, 3.1, 3) * 0.05;
      const d = (v - wob * 1.0) / (width / H);
      const band = Math.exp(-d * d * 1.6);
      if (band < 0.01) continue;
      const cloud = 0.5 + 0.5 * n.fbm(u * 6, v * 9 + 40, 5);
      const dust = smoothstep(0.52, 0.72, 0.5 + 0.5 * n.fbm(u * 5 + 13, v * 14, 5)) * Math.exp(-d * d * 6);
      const lum = band * (0.35 + 0.9 * cloud * cloud) * (1 - 0.8 * dust);
      const core = Math.exp(-d * d * 5);
      const k = (j * lw + i) * 4;
      img.data[k] = 205 + 40 * core;
      img.data[k + 1] = 210 + 22 * core;
      img.data[k + 2] = 255 - 40 * core;
      img.data[k + 3] = Math.min(255, lum * 150 * l.milkyWay);
    }
  }
  const tile = makeCanvas(lw, lh);
  context(tile).putImageData(img, 0, 0);
  c.save();
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.globalCompositeOperation = "lighter";
  c.globalAlpha = 0.55;
  c.drawImage(tile as unknown as CanvasImageSource, 0, 0, W, HORIZON * H);
  c.restore();
  // Its stars: many faint ones gathered along the band.
  const count = Math.round(1100 * l.milkyWay * (W / H) / 1.78);
  for (let i = 0; i < count; i++) {
    const t = r();
    const g = (r() + r() + r() - 1.5) * 0.9;
    const x = ax + dx * t + nx * g * width;
    const y = ay + dy * t + ny * g * width;
    if (y > HORIZON * H || y < 0 || x < 0 || x > W) continue;
    const a = (0.15 + 0.5 * Math.pow(r(), 3)) * l.milkyWay * (1 - Math.abs(g) * 0.5);
    c.fillStyle = css(starColor(r), a);
    const s = (0.5 + r() * 0.7) * f.px;
    c.fillRect(x, y, s, s);
  }
}

// ── The moon and the sun ──────────────────────────────────────────────────────

export function paintMoon(c: Ctx, f: Frame) {
  const m = f.spec.moon;
  if (!m || !f.moon) return;
  const { W, H, light: l } = f;
  const x = f.moon.x * W;
  const y = f.moon.y * H;
  const R = 0.026 * H;
  const fullness = (1 - Math.cos(2 * Math.PI * m.phase)) / 2;
  // The halo, brighter as the moon fills.
  const halo = c.createRadialGradient(x, y, R * 0.8, x, y, R * 11);
  halo.addColorStop(0, css(hex("#dfe6ff"), 0.22 * (0.3 + fullness)));
  halo.addColorStop(0.25, css(hex("#b9c6ef"), 0.08 * (0.3 + fullness)));
  halo.addColorStop(1, css(hex("#b9c6ef"), 0));
  c.fillStyle = halo;
  c.fillRect(x - R * 11, y - R * 11, R * 22, R * 22);

  c.save();
  c.translate(x, y);
  // The lit limb faces the sun, wherever it is below the horizon.
  c.rotate(Math.atan2(f.sun.y * H - y, f.sun.x * W - x));
  // Earthshine on the dark side.
  c.fillStyle = css(mix(l.zenith, hex("#8a96b8"), 0.22), 0.9);
  c.beginPath();
  c.arc(0, 0, R, 0, Math.PI * 2);
  c.fill();
  const lit = new Path2D();
  const k = Math.cos(2 * Math.PI * m.phase);
  lit.arc(0, 0, R, -Math.PI / 2, Math.PI / 2);
  if (k >= 0) lit.ellipse(0, 0, Math.max(0.01, R * k), R, 0, Math.PI / 2, -Math.PI / 2, true);
  else lit.ellipse(0, 0, Math.max(0.01, -R * k), R, 0, Math.PI / 2, Math.PI * 1.5, false);
  lit.closePath();
  const g = c.createRadialGradient(-R * 0.25, -R * 0.2, 0, 0, 0, R);
  g.addColorStop(0, "#fbf8ee");
  g.addColorStop(0.75, "#ece6d6");
  g.addColorStop(1, "#d4cfc2");
  c.fillStyle = g;
  c.fill(lit);
  // Maria: soft grey seas, the same every night.
  c.save();
  c.clip(lit);
  const r = rng(0x4d00);
  for (let i = 0; i < 9; i++) {
    const mx = (r() - 0.5) * R * 1.3;
    const my = (r() - 0.5) * R * 1.3;
    const mr = R * (0.15 + r() * 0.3);
    const sea = c.createRadialGradient(mx, my, 0, mx, my, mr);
    sea.addColorStop(0, "rgba(120,120,135,0.22)");
    sea.addColorStop(1, "rgba(120,120,135,0)");
    c.fillStyle = sea;
    c.fillRect(mx - mr, my - mr, mr * 2, mr * 2);
  }
  c.restore();
  c.restore();
}

export function paintSun(c: Ctx, f: Frame) {
  const { W, H, light: l, spec } = f;
  if (spec.theme === "space" || spec.sun.elevation < -1.2 || spec.afterglow) return;
  const x = f.sun.x * W;
  const y = f.sun.y * H;
  const low = clamp01(1 - spec.sun.elevation / 8);
  // A low sun looks bigger.
  const R = 0.021 * H * (1 + 0.35 * low);
  const veiled = spec.theme === "cloudy" ? 0.55 : spec.theme === "storm" ? 0.25 : 1;
  // Aureole.
  const a = c.createRadialGradient(x, y, R * 0.6, x, y, R * 9);
  a.addColorStop(0, css(mix(l.sunColor, [255, 255, 255], 0.5), 0.7 * veiled));
  a.addColorStop(0.18, css(l.sunColor, 0.32 * veiled));
  a.addColorStop(1, css(l.sunColor, 0));
  c.fillStyle = a;
  c.fillRect(x - R * 9, y - R * 9, R * 18, R * 18);
  // Disc, a little squashed by the air near the horizon.
  c.save();
  c.translate(x, y);
  c.scale(1, 1 - 0.1 * low);
  const d = c.createRadialGradient(0, 0, 0, 0, 0, R);
  d.addColorStop(0, css(mix(l.sunColor, [255, 255, 250], 0.75), veiled));
  d.addColorStop(0.75, css(mix(l.sunColor, [255, 255, 250], 0.35), veiled));
  d.addColorStop(1, css(l.sunColor, 0.85 * veiled));
  c.fillStyle = d;
  c.beginPath();
  c.arc(0, 0, R, 0, Math.PI * 2);
  c.fill();
  c.restore();
}

// ── The high cloud deck ───────────────────────────────────────────────────────

/**
 * A layer of cloud on a plane above the viewer, seen in perspective: wide and
 * soft overhead, ever thinner toward the horizon. Thin and glowing at sunset,
 * a grey roof when cloudy, a dark lid in a storm.
 */
export function paintDeck(c: Ctx, f: Frame) {
  const { W, H, light: l, noise: n } = f;
  if (l.deck <= 0.01) return;
  const lw = Math.max(96, Math.min(720, Math.round(W / 4.5)));
  const lh = Math.max(32, Math.round((lw * H * HORIZON) / W));
  const img = new ImageData(lw, lh);
  const aspect = W / H;
  const theme = f.spec.theme;
  const streaky = theme === "golden" || theme === "sunset" || theme === "clear" || theme === "night";
  const stretch = streaky ? 0.45 : 0.85;
  const th = 0.78 - l.deck * 0.5;
  const soft = streaky ? 0.16 : 0.2;
  const octaves = theme === "storm" ? 6 : 5;
  const sx = f.sun.x;
  const sy = Math.min(f.sun.y, 1.2);
  const low = clamp01(1 - f.spec.sun.elevation / 10) * (f.spec.sun.elevation > -9 ? 1 : 0);
  const lit = l.lit;
  const shade = l.shade;
  const mid = l.cmid;
  const glow = mix(l.glow, l.sunColor, 0.3);
  const ox = (f.spec.seed % 997) * 0.37;
  for (let j = 0; j < lh; j++) {
    const v = ((j + 0.5) / lh) * HORIZON;
    const dist = 1 / (HORIZON - v + 0.04);
    const haze = smoothstep(0.35, HORIZON, v);
    for (let i = 0; i < lw; i++) {
      const u = (i + 0.5) / lw;
      const X = (u - 0.5) * aspect * dist * 0.8 * stretch + ox;
      const Y = dist * 1.6;
      const base = 0.5 + 0.5 * n.fbm(X, Y, octaves);
      const dens = smoothstep(th - soft, th + soft, base + (streaky ? 0.12 * n.noise(X * 0.35, Y * 3.5) : 0));
      if (dens < 0.01) continue;
      // Thin cloud lets light through; thick cloud is dark underneath, and
      // mottled where it heaps up (the undersides of stratocumulus).
      const thick = dens * l.deckThick;
      let col = mix(lit, mid, Math.min(1, thick * 1.2));
      col = mix(col, shade, Math.max(0, thick - 0.45) * 1.4);
      if (!streaky) {
        const heap = n.fbm(X * 3.1 + 40, Y * 3.1, 4);
        col = mix(col, heap > 0 ? shade : lit, Math.min(1, Math.abs(heap) * 0.9 * thick));
      }
      // Lit from below near a low sun: the glowing undersides of a sunset.
      const du = (u - sx) * aspect;
      const dv = v - sy;
      const near = Math.exp(-(du * du * 0.9 + dv * dv * 3));
      const under = low * Math.exp(-du * du * 0.35) * (0.35 + 0.65 * (v / HORIZON));
      col = mix(col, glow, Math.min(1, near * l.glowAmt * 0.9 + under * 0.55 * (1 - thick * 0.5)));
      col = mix(col, l.horizon, haze * 0.55);
      const k = (j * lw + i) * 4;
      img.data[k] = col[0];
      img.data[k + 1] = col[1];
      img.data[k + 2] = col[2];
      // Far away the deck thins out, so a low sun shows under it.
      img.data[k + 3] = 255 * dens * (0.5 + 0.5 * l.deckThick) * (1 - haze * (streaky ? 0.75 : 0.45));
    }
  }
  const tile = makeCanvas(lw, lh);
  context(tile).putImageData(img, 0, 0);
  c.save();
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.drawImage(tile as unknown as CanvasImageSource, 0, 0, W, HORIZON * H);
  c.restore();
}

// ── Lobed clouds (the cumulus and the sea of clouds) ──────────────────────────

/** A heap of cloud: a sphere on a grid, `z` toward the viewer. */
interface Lobe {
  x: number;
  y: number;
  r: number;
  z: number;
  /** Vertical squash (1: round); far rows of the sea are flattened by perspective. */
  sy?: number;
}

interface LobeColors {
  lit: RGB;
  mid: RGB;
  shade: RGB;
  glow: RGB;
}

interface LobeLight {
  /** The light and the sun, in grid coordinates. */
  light: Pt;
  sun: Pt;
  /** Distance (grid px) within which the sun backlights the cloud; 0: never. */
  backlitRadius: number;
  /** Strength of the light (night is dim). */
  amt: number;
  /** Extra silver lining (0…1). */
  rim: number;
}

/** Height-field value at a grid point, the nearest edge outside (for slopes). */
const edgeAt = (D: Float32Array, gw: number, gh: number, x: number, y: number) =>
  D[Math.min(gh - 1, Math.max(0, Math.round(y))) * gw + Math.min(gw - 1, Math.max(0, Math.round(x)))];

/** Height-field value at a grid point (0 outside). */
const at = (D: Float32Array, gw: number, gh: number, x: number, y: number) => {
  const xi = Math.round(x);
  const yi = Math.round(y);
  return xi < 0 || yi < 0 || xi >= gw || yi >= gh ? 0 : D[yi * gw + xi];
};

/**
 * Shades a union of lobes into RGBA, per pixel. Each lobe catches the light
 * on its own and the creases between them fall into shade: normals from the
 * surface's height, the sun from its direction on screen and from our side
 * (or from behind, near the sun: dark bodies, glowing edges), self-shadowing
 * by the lobes, sky light from above, fractal bumps and ragged edges.
 * `base`: a flat cut-off below (a cumulus); `fill`: solid below (a sea row).
 */
function shadeLobes(
  lobes: Lobe[], gw: number, gh: number, scale: number, n: Noise, seed: number,
  colors: LobeColors, li: LobeLight, opts: { base?: number; fill?: number; dim?: number },
): ImageData {
  const Z = new Float32Array(gw * gh);
  const C = new Float32Array(gw * gh);
  for (const p of lobes) {
    const x0 = Math.max(0, Math.floor(p.x - p.r));
    const x1 = Math.min(gw - 1, Math.ceil(p.x + p.r));
    const y0 = Math.max(0, Math.floor(p.y - p.r));
    const y1 = Math.min(gh - 1, Math.ceil(p.y + p.r));
    const rr = p.r * p.r;
    const sy = p.sy ?? 1;
    for (let y = y0; y <= y1; y++) {
      const dy = (y - p.y) / sy;
      for (let x = x0; x <= x1; x++) {
        const q = ((x - p.x) * (x - p.x) + dy * dy) / rr;
        if (q >= 1) continue;
        const i = y * gw + x;
        // Lobes are flatter than spheres: soft heaps, not balls.
        const z = p.z + p.r * 0.62 * Math.sqrt(1 - q);
        if (z > Z[i]) Z[i] = z;
        if (1 - q > C[i]) C[i] = 1 - q;
      }
    }
  }
  if (opts.fill != null) {
    // Solid below a wavy line, rising gently from it (no ledge).
    const ramp = scale * 0.12;
    for (let x = 0; x < gw; x++) {
      const edge = opts.fill + n.noise(x / (scale * 0.18), seed % 89) * scale * 0.06;
      for (let y = Math.max(0, Math.floor(edge)); y < gh; y++) {
        const i = y * gw + x;
        const k = smoothstep(edge, edge + ramp, y);
        C[i] = Math.max(C[i], k);
        Z[i] = Math.max(Z[i], scale * 0.15 * k);
      }
    }
  }
  const r = rng(seed);
  const ox = r() * 100;
  const oy = r() * 100;
  const A = new Float32Array(gw * gh);
  for (let y = 0; y < gh; y++) {
    const flat = opts.base != null ? smoothstep(opts.base + scale * 0.01, opts.base - scale * 0.03, y) : 1;
    for (let x = 0; x < gw; x++) {
      const i = y * gw + x;
      if (C[i] <= 0) continue;
      const fine = n.fbm(ox + (x / scale) * 9, oy + (y / scale) * 9, 4);
      A[i] = smoothstep(0.05, 0.2, C[i] + (fine - 0.1) * 0.16) * flat;
      Z[i] += n.fbm(oy + (x / scale) * 6, ox + (y / scale) * 6, 4) * scale * 0.03;
    }
  }
  // Light diffuses inside a cloud: shade from a softened surface, so creases
  // between lobes are valleys, not cracks.
  const S = blur(Z, gw, gh, Math.max(1, Math.round(scale * 0.022)));
  const img = new ImageData(gw, gh);
  const step = Math.max(1, scale * 0.015);
  const dim = opts.dim ?? 0.3;
  for (let y = 0; y < gh; y++) {
    const low = opts.base != null ? smoothstep(opts.base - scale * 0.32, opts.base, y) : 0;
    const deep = opts.fill != null ? Math.max(0, y - opts.fill) / (scale * 0.25) : 0;
    for (let x = 0; x < gw; x++) {
      const i = y * gw + x;
      const a = A[i];
      if (a < 0.01) continue;
      // Where the light comes from, for this point.
      let Lx = li.light.x - x;
      let Ly = li.light.y - y;
      const l2 = Math.hypot(Lx, Ly) || 1;
      Lx /= l2;
      Ly /= l2;
      const near = li.backlitRadius > 0
        ? Math.exp(-((li.sun.x - x) ** 2 + (li.sun.y - y) ** 2) / (li.backlitRadius * li.backlitRadius))
        : 0;
      const front = 0.75 - 1.05 * near;
      const len3 = Math.hypot(1, front);
      const lx = Lx / len3;
      const ly = Ly / len3;
      const lz = front / len3;
      const z = Z[i];
      const zx = (edgeAt(S, gw, gh, x + 1.5, y) - edgeAt(S, gw, gh, x - 1.5, y)) / 3;
      const zy = (edgeAt(S, gw, gh, x, y + 1.5) - edgeAt(S, gw, gh, x, y - 1.5)) / 3;
      const nl = Math.hypot(zx, zy, 1);
      const nx = -zx / nl;
      const ny = -zy / nl;
      const ndl = nx * lx + ny * ly + lz / nl;
      const wrap = Math.max(0, (ndl + 0.4) / 1.4);
      let occ = 0;
      for (let s = 1; s <= 8; s++) {
        const dist = s * step;
        occ += Math.max(0, at(Z, gw, gh, x + Lx * dist, y + Ly * dist) - (z + front * dist));
      }
      const shadow = Math.exp(-occ / (scale * 0.08));
      const sky = 0.5 - 0.5 * ny;
      let light = 0.12 + 0.14 * sky + 0.85 * wrap * shadow * li.amt;
      light *= (1 - (0.2 + dim) * low) * (1 - 0.45 * Math.min(1, deep));
      let col = light < 0.55
        ? mix(colors.shade, colors.mid, Math.max(0, light) / 0.55)
        : mix(colors.mid, colors.lit, Math.min(1, (light - 0.55) / 0.35));
      // Thin edges glow when the light is behind.
      const thin = 1 - smoothstep(0, 0.45, C[i]);
      const glowAmt = thin * Math.min(1, near * 1.2 + li.rim) * (0.55 + 0.45 * Math.max(0, nx * Lx + ny * Ly));
      col = mix(col, colors.glow, Math.min(1, glowAmt));
      const o = i * 4;
      img.data[o] = col[0];
      img.data[o + 1] = col[1];
      img.data[o + 2] = col[2];
      img.data[o + 3] = 255 * a;
    }
  }
  return img;
}

/** A box blur of `radius` cells (two passes, horizontal then vertical). */
function blur(src: Float32Array, w: number, h: number, radius: number): Float32Array {
  const tmp = new Float32Array(src.length);
  const out = new Float32Array(src.length);
  const span = radius * 2 + 1;
  for (let y = 0; y < h; y++) {
    let sum = 0;
    const row = y * w;
    for (let x = -radius; x <= radius; x++) sum += src[row + Math.min(w - 1, Math.max(0, x))];
    for (let x = 0; x < w; x++) {
      tmp[row + x] = sum / span;
      sum += src[row + Math.min(w - 1, x + radius + 1)] - src[row + Math.max(0, x - radius)];
    }
  }
  for (let x = 0; x < w; x++) {
    let sum = 0;
    for (let y = -radius; y <= radius; y++) sum += tmp[Math.min(h - 1, Math.max(0, y)) * w + x];
    for (let y = 0; y < h; y++) {
      out[y * w + x] = sum / span;
      sum += tmp[Math.min(h - 1, y + radius + 1) * w + x] - tmp[Math.max(0, y - radius) * w + x];
    }
  }
  return out;
}

/** An ImageData scaled up once, smoothly, to `w`×`h`, so a frame only copies it. */
function upscale(img: ImageData, w: number, h: number): AnyCanvas {
  const tile = makeCanvas(img.width, img.height);
  context(tile).putImageData(img, 0, 0);
  const out = makeCanvas(w, h);
  const c = context(out);
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.drawImage(tile as unknown as CanvasImageSource, 0, 0, out.width, out.height);
  return out;
}

// ── Cumulus ───────────────────────────────────────────────────────────────────

export interface Cumulus {
  sprite: AnyCanvas;
  /** On screen, pixels. */
  w: number;
  h: number;
  /** Start position as a fraction of the drift loop, and the top of the sprite. */
  x0: number;
  y: number;
  /** Pixels per second. */
  speed: number;
}

/** Where the flat base of a cumulus sits in its box. */
const CUMULUS_BASE = 0.84;

/** The heaps of one cumulus in a w×h box: a row along the flat base, a dome, turrets on the crown. */
function cumulusLobes(r: () => number, w: number, h: number): Lobe[] {
  const out: Lobe[] = [];
  const base = h * CUMULUS_BASE;
  const env = (u: number) => Math.pow(Math.max(0, 1 - Math.pow(Math.abs(2 * u - 1), 2)), 0.7);
  const nBase = 4 + Math.floor(r() * 3);
  for (let i = 0; i < nBase; i++) {
    const rad = w * (0.11 + r() * 0.05);
    out.push({ x: w * (0.17 + (0.66 * (i + 0.5)) / nBase) + (r() - 0.5) * w * 0.06, y: base - rad * 0.4, r: rad, z: rad * 0.35 * r() });
  }
  const nDome = 9 + Math.floor(r() * 6);
  for (let i = 0; i < nDome; i++) {
    const u = (r() + r()) / 2;
    const top = base - h * 0.7 * env(u);
    const y = base - (base - top) * (0.3 + 0.55 * r());
    const rad = w * (0.06 + 0.08 * env(u)) * (0.8 + 0.4 * r());
    out.push({ x: w * (0.12 + 0.76 * u), y, r: rad, z: rad * 0.35 * r() });
  }
  const nTop = 6 + Math.floor(r() * 6);
  for (let i = 0; i < nTop; i++) {
    const u = 0.2 + 0.6 * r();
    const rad = w * (0.04 + 0.035 * r());
    out.push({ x: w * u, y: base - h * 0.7 * env(u) + w * 0.03, r: rad, z: rad * 0.35 * r() });
  }
  return out;
}

/** The scene's cumulus, placed away from Ciel (and from the clock on the lock screen). */
export function buildCumulus(f: Frame): Cumulus[] {
  const { W, H, light: l, spec } = f;
  const r = rng(spec.seed ^ 0xc0c0);
  const count = Math.round(l.cumulus * Math.min(1.6, Math.max(0.7, W / H / 1.78)));
  const out: Cumulus[] = [];
  const ciel = spec.ciel;
  const cielTop = ciel ? ciel.y - ciel.size * 0.55 : 2;
  const cielBottom = ciel ? ciel.y + ciel.size * 0.45 : 2;
  const tower = spec.theme === "storm" ? 1.7 : spec.theme === "cloudy" ? 1.15 : 1;
  const backlit = spec.sun.elevation > -8 && !spec.afterglow && spec.theme !== "night" && spec.theme !== "space";
  for (let i = 0; i < count; i++) {
    const depth = (i + r()) / count;
    const w = (0.15 + 0.36 * Math.pow(depth, 1.4)) * l.cumulusSize * (0.8 + 0.4 * r()) * H;
    const hgt = w * (0.4 + 0.14 * r()) * tower;
    let base = HORIZON - 0.05 - depth * 0.5 + (r() - 0.5) * 0.06;
    for (let k = 0; k < 6; k++) {
      const top = base - hgt / H;
      const overlapsCiel = base > cielTop && top < cielBottom;
      const overClock = spec.lockScreen && top < 0.4 && base > 0.08;
      if (!overlapsCiel && !overClock) break;
      base = 0.15 + r() * (HORIZON - 0.2);
      if (k === 5 && overlapsCiel && cielTop - 0.03 > hgt / H + 0.06) base = cielTop - 0.03;
    }
    base = Math.min(HORIZON - 0.02, Math.max(hgt / H + 0.03, base));
    // Farther clouds melt into the haze.
    const haze = (1 - depth) * 0.45;
    const colors: LobeColors = {
      lit: mix(l.lit, l.horizon, haze * 0.6),
      mid: mix(l.cmid, l.horizon, haze),
      shade: mix(l.shade, l.horizon, haze),
      glow: mix(l.glow, l.sunColor, 0.5),
    };
    const cx = r() * W;
    // The grid: up to 380 cells wide, with a margin of 6 % on each side.
    const G = Math.max(24, Math.min(380, Math.round(w / 1.5)));
    const k = G / w;
    const pad = Math.round(G * 0.06);
    const gw = G + pad * 2;
    const gh = Math.round(hgt * k) + pad * 2;
    const lobes = cumulusLobes(r, G, hgt * k).map((p) => ({ ...p, x: p.x + pad, y: p.y + pad }));
    // Grid ↔ screen, for where the light is.
    const left = cx - w / 2 - pad / k;
    const topPx = base * H - hgt * CUMULUS_BASE - pad / k;
    const toGrid = (p: Pt): Pt => ({ x: (p.x - left) * k, y: (p.y - topPx) * k });
    const img = shadeLobes(lobes, gw, gh, G, f.noise, spec.seed + i * 101, colors, {
      light: toGrid(f.lightAt),
      sun: toGrid({ x: f.sun.x * W, y: f.sun.y * H }),
      backlitRadius: backlit ? 0.45 * H * k : 0,
      amt: Math.max(0.3, f.lightAmt),
      rim: l.rimAmt * 0.25,
    }, { base: pad + hgt * k * CUMULUS_BASE, dim: spec.theme === "storm" ? 0.45 : 0.3 });
    if (spec.theme === "night") for (let j = 3; j < img.data.length; j += 4) img.data[j] *= 0.85;
    const sw = gw / k;
    const sh = gh / k;
    out.push({
      sprite: upscale(img, Math.min(Math.round(sw), gw * 2.2), Math.min(Math.round(sh), gh * 2.2)),
      w: sw,
      h: sh,
      x0: left / (W + sw),
      y: topPx,
      speed: (0.004 + 0.009 * depth) * H,
    });
  }
  return out;
}

/** The cumulus drifting at time `t` (seconds). */
export function paintCumulus(c: Ctx, clouds: Cumulus[], W: number, t: number) {
  c.save();
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "medium";
  for (const cl of clouds) {
    const loop = W + cl.w;
    const x = ((((cl.x0 * loop + cl.speed * t) % loop) + loop) % loop) - cl.w;
    c.drawImage(cl.sprite as unknown as CanvasImageSource, x, cl.y, cl.w, cl.h);
  }
  c.restore();
}

// ── The sea of clouds ─────────────────────────────────────────────────────────

/**
 * The tops of a layer of cloud below the horizon, row behind row in
 * perspective, each row a line of lobes shaded like the cumulus, deeper rows
 * darker underneath and farther rows paler in the haze. Returns a strip from
 * `top` (pixels) to the bottom of the screen.
 */
export function paintCloudSea(f: Frame): { canvas: AnyCanvas; top: number } {
  const { W, H, light: l, spec } = f;
  const top = Math.floor((HORIZON - 0.05) * H);
  const k = Math.min(1, 1100 / W);
  const gw = Math.max(16, Math.round(W * k));
  const gh = Math.max(4, Math.round((H - top) * k));
  const strip = makeCanvas(W, H - top);
  const c = context(strip);
  const r = rng(spec.seed ^ 0x5ea);
  const rows = 6;
  const toGrid = (p: Pt): Pt => ({ x: p.x * k, y: (p.y - top) * k });
  const backlit = spec.sun.elevation > -8 && !spec.afterglow && spec.theme !== "night";
  for (let i = 0; i < rows; i++) {
    const p = i / (rows - 1);
    const rowY = ((HORIZON + 0.006 + 0.25 * Math.pow(p, 1.6)) * H - top) * k;
    const rad = (0.012 + 0.085 * Math.pow(p, 1.5)) * H * k;
    const lobes: Lobe[] = [];
    // Lobes wider than tall, flatter with distance.
    const sy = 0.55 + 0.3 * p;
    let x = -rad;
    while (x < gw + rad) {
      const rr = rad * (0.6 + 0.7 * r());
      lobes.push({ x, y: rowY - rr * sy * (0.1 + 0.45 * r()), r: rr, z: rr * 0.3 * r(), sy });
      // Smaller heaps on top, and a second rank underneath so no gap shows.
      if (r() < 0.55) {
        const sr = rr * (0.4 + 0.3 * r());
        lobes.push({ x: x + (r() - 0.5) * rr, y: rowY - rr * sy * 0.6 - sr * sy * 0.2, r: sr, z: rr * 0.35, sy });
      }
      lobes.push({ x: x + rr * 0.5, y: rowY + rr * sy * 0.55, r: rr * 1.1, z: -rr * 0.2, sy });
      x += rr * (0.8 + 0.5 * r());
    }
    const haze = Math.pow(1 - p, 1.5) * 0.7;
    const colors: LobeColors = {
      lit: mix(l.lit, l.horizon, haze * 0.7),
      mid: mix(mix(l.cmid, l.shade, 0.2), l.horizon, haze),
      shade: mix(mix(l.shade, l.zenith, 0.2), l.horizon, haze),
      glow: mix(l.glow, l.sunColor, 0.4),
    };
    // The nearest row's body reaches the bottom of the screen everywhere.
    const fill = i === rows - 1 ? gh - rad * 0.35 : rowY + rad * 1.1;
    const img = shadeLobes(lobes, gw, gh, rad * 4, f.noise, spec.seed + 977 * i, colors, {
      light: toGrid(f.lightAt),
      sun: toGrid({ x: f.sun.x * W, y: f.sun.y * H }),
      backlitRadius: backlit ? 0.35 * H * k : 0,
      amt: Math.max(0.3, f.lightAmt),
      rim: 0,
    }, { fill });
    const layer = upscale(img, W, H - top);
    c.drawImage(layer as unknown as CanvasImageSource, 0, 0);
  }
  // Sunlight pooling on the tops nearest the sun, and haze over the farthest rows.
  c.globalCompositeOperation = "source-atop";
  if (l.glowAmt > 0.05 && spec.theme !== "space") {
    const gx = f.sun.x * W;
    const g = c.createRadialGradient(gx, 0, 0, gx, 0, W * 0.4);
    g.addColorStop(0, css(mix(l.glow, l.sunColor, 0.4), 0.45 * l.glowAmt));
    g.addColorStop(1, css(l.glow, 0));
    c.fillStyle = g;
    c.fillRect(0, 0, W, H - top);
  }
  const hz = c.createLinearGradient(0, 0, 0, 0.1 * H);
  hz.addColorStop(0, css(l.horizon, 0.5));
  hz.addColorStop(1, css(l.horizon, 0));
  c.fillStyle = hz;
  c.fillRect(0, 0, W, 0.1 * H);
  return { canvas: strip, top };
}

// ── Rain, lightning, meteors ──────────────────────────────────────────────────

export function paintRain(c: Ctx, f: Frame, t: number) {
  const { W, H, light: l, px } = f;
  if (l.rain <= 0) return;
  const r = rng(f.spec.seed ^ 0x7a1);
  const count = Math.round(320 * l.rain * (W / H) / 1.78);
  const slant = 0.16;
  c.save();
  c.strokeStyle = css(mix(l.lit, [220, 230, 245], 0.5), 0.22);
  c.lineWidth = Math.max(0.8, 1.3 * px);
  c.lineCap = "round";
  c.beginPath();
  const span = H * 1.15;
  for (let i = 0; i < count; i++) {
    const x0 = r() * (W + span * slant);
    const y0 = r() * span;
    const len = (0.02 + r() * 0.035) * H;
    const speed = (0.9 + r() * 0.7) * H;
    const y = ((y0 + speed * t) % span) - 0.1 * H;
    const x = x0 - y * slant;
    c.moveTo(x, y);
    c.lineTo(x - len * slant, y + len);
  }
  c.stroke();
  c.restore();
}

/** A forked bolt from the cloud base toward the horizon (seeded), as points. */
export function boltPath(f: Frame, which: number): Pt[][] {
  const { W, H } = f;
  const r = rng(f.spec.seed ^ (0xb017 + which * 977));
  const side = r() < 0.5 ? 0.12 + r() * 0.22 : 0.66 + r() * 0.22;
  const main: Pt[] = [];
  let x = side * W;
  let y = 0.22 * H;
  const end = HORIZON * H + 0.02 * H;
  while (y < end) {
    main.push({ x, y });
    y += (0.015 + r() * 0.03) * H;
    x += (r() - 0.5) * 0.05 * H;
  }
  main.push({ x, y: end });
  const branches: Pt[][] = [main];
  for (let b = 0; b < 3; b++) {
    const from = main[2 + Math.floor(r() * (main.length - 4))];
    if (!from) continue;
    const br: Pt[] = [from];
    let bx = from.x;
    let by = from.y;
    const dir = r() < 0.5 ? -1 : 1;
    for (let i = 0; i < 4 + Math.floor(r() * 4); i++) {
      by += (0.012 + r() * 0.02) * H;
      bx += dir * (0.005 + r() * 0.025) * H;
      br.push({ x: bx, y: by });
    }
    branches.push(br);
  }
  return branches;
}

/** Lightning: a glow in the clouds and the bolt itself, at strength `k` (0…1). */
export function paintLightning(c: Ctx, f: Frame, bolt: Pt[][], k: number) {
  if (k <= 0.01 || bolt.length === 0) return;
  const { W, H, px } = f;
  const start = bolt[0][0];
  c.save();
  c.globalCompositeOperation = "lighter";
  const g = c.createRadialGradient(start.x, start.y, 0, start.x, start.y, H * 0.6);
  g.addColorStop(0, `rgba(200,210,255,${0.35 * k})`);
  g.addColorStop(1, "rgba(200,210,255,0)");
  c.fillStyle = g;
  c.fillRect(0, 0, W, HORIZON * H);
  c.lineCap = "round";
  c.lineJoin = "round";
  for (const [width, alpha] of [[9, 0.12], [4, 0.35], [1.6, 0.95]] as const) {
    c.strokeStyle = `rgba(225,232,255,${alpha * k})`;
    bolt.forEach((line, i) => {
      c.lineWidth = width * px * (i === 0 ? 1 : 0.55);
      c.beginPath();
      line.forEach((p, j) => (j === 0 ? c.moveTo(p.x, p.y) : c.lineTo(p.x, p.y)));
      c.stroke();
    });
  }
  c.restore();
}

/** How bright the lightning is at `t`: a rare single flash, never a flicker. */
export function lightningAt(seed: number, t: number): { k: number; which: number } {
  const period = 13;
  const n = Math.floor(t / period);
  const at = n * period + hash01(seed, n) * (period - 2);
  const d = t - at;
  if (hash01(seed, n, 3) < 0.35 || d < 0 || d > 0.9) return { k: 0, which: n };
  return { k: d < 0.08 ? d / 0.08 : Math.exp(-(d - 0.08) * 6), which: n };
}

/** A shooting star now and then on clear nights and in space. */
export function paintMeteor(c: Ctx, f: Frame, t: number) {
  const period = 23;
  const n = Math.floor(t / period);
  const seed = f.spec.seed;
  if (hash01(seed, n, 9) < 0.4) return;
  const start = n * period + hash01(seed, n, 1) * (period - 2);
  const d = (t - start) / 0.9;
  if (d < 0 || d > 1) return;
  const { W, H, px } = f;
  const x0 = (0.15 + 0.7 * hash01(seed, n, 2)) * W;
  const y0 = (0.05 + 0.3 * hash01(seed, n, 4)) * H;
  const dir = hash01(seed, n, 5) < 0.5 ? -1 : 1;
  const len = 0.22 * H;
  const hx = x0 + dir * len * d;
  const hy = y0 + len * 0.45 * d;
  const tail = 0.5 * len * Math.sin(Math.PI * d);
  const a = Math.sin(Math.PI * d);
  const g = c.createLinearGradient(hx, hy, hx - dir * tail, hy - tail * 0.45);
  g.addColorStop(0, `rgba(255,255,255,${0.9 * a})`);
  g.addColorStop(1, "rgba(255,255,255,0)");
  c.save();
  c.strokeStyle = g;
  c.lineWidth = 1.6 * px;
  c.lineCap = "round";
  c.beginPath();
  c.moveTo(hx, hy);
  c.lineTo(hx - dir * tail, hy - tail * 0.45);
  c.stroke();
  c.restore();
}

// ── Space ─────────────────────────────────────────────────────────────────────

const NEBULAE: [RGB, RGB, RGB][] = [
  // gas, glowing core, second gas
  [hex("#1fb3a8"), hex("#ffd9a0"), hex("#e8873f")],
  [hex("#d03a8c"), hex("#ffd0ec"), hex("#3f6dff")],
  [hex("#ff5d86"), hex("#ffe1e6"), hex("#7357ff")],
];

/**
 * Deep space in the spirit of the great telescope pictures: a nebula of
 * glowing gas with dark dust, a dense star field, a few bright stars with
 * diffraction spikes, and a distant planet.
 */
export function paintSpace(c: Ctx, f: Frame): Star[] {
  const { W, H } = f;
  const r = rng(f.spec.seed ^ 0x5bace);
  const bg = c.createRadialGradient(W * 0.62, H * 0.42, 0, W * 0.62, H * 0.42, Math.max(W, H) * 0.8);
  bg.addColorStop(0, "#0b1030");
  bg.addColorStop(0.45, "#05071a");
  bg.addColorStop(1, "#010106");
  c.fillStyle = bg;
  c.fillRect(0, 0, W, H);

  paintNebula(c, f, NEBULAE[Math.floor(r() * NEBULAE.length)], r);
  const twinklers = paintStars(c, f, 2.4, 1);
  paintSpikes(c, f, r);
  paintPlanet(c, f, r);
  return twinklers;
}

function paintNebula(c: Ctx, f: Frame, colors: [RGB, RGB, RGB], r: () => number) {
  const { W, H, noise: n } = f;
  const lw = Math.max(96, Math.min(820, Math.round(W / 4)));
  const lh = Math.max(40, Math.round((lw * H) / W));
  const img = new ImageData(lw, lh);
  const aspect = W / H;
  // A drifting ribbon of gas across the frame.
  const cx = 0.45 + r() * 0.3;
  const cy = 0.35 + r() * 0.25;
  const angle = (r() - 0.5) * 1.4;
  const ca = Math.cos(angle);
  const sa = Math.sin(angle);
  const [gas, core, gas2] = colors;
  for (let j = 0; j < lh; j++) {
    const v = (j + 0.5) / lh;
    for (let i = 0; i < lw; i++) {
      const u = (i + 0.5) / lw;
      const x = (u - cx) * aspect;
      const y = v - cy;
      const along = x * ca + y * sa;
      const across = -x * sa + y * ca;
      const px = u * aspect * 1.6;
      const py = v * 1.6;
      const qx = n.fbm(px, py, 4);
      const qy = n.fbm(px + 5.2, py + 1.3, 4);
      const wob = n.fbm(along * 1.5, 7.7, 3) * 0.3;
      const m = Math.exp(-Math.pow((across - wob) / 0.38, 2)) * Math.exp(-Math.pow(along / 1.5, 2));
      if (m < 0.01) continue;
      const g0 = 0.5 + 0.5 * n.fbm(px * 0.7 + 3, py * 0.7, 3);
      const g1 = 0.5 + 0.5 * n.fbm(px * 1.4 + qx * 1.1, py * 1.4 + qy * 1.1, 6);
      const g2 = 0.5 + 0.5 * n.fbm(px * 2.2 - qy * 2 + 11, py * 2.2 + qx * 2, 5);
      const d = 0.5 + 0.5 * n.fbm(px * 2.6 + qx * 2.5 + 23, py * 2.6 - qy * 2.5, 6);
      // A faint wide glow, bright gas in filaments, a second gas, a hot core.
      const a0 = smoothstep(0.25, 0.8, g0) * m * 0.3;
      const a1 = smoothstep(0.34, 0.82, g1) * m;
      const a2 = smoothstep(0.5, 0.92, g2) * Math.pow(m, 1.2);
      const hot = smoothstep(0.68, 0.95, g1) * Math.pow(m, 1.8);
      const dk = smoothstep(0.48, 0.76, d) * Math.min(1, m * 1.6);
      const glow = (1 - dk * 0.88) * 0.82;
      const col: RGB = [
        (gas[0] * (a0 + a1 * 1.1) + gas2[0] * a2 * 0.95 + core[0] * hot * 0.9) * glow,
        (gas[1] * (a0 + a1 * 1.1) + gas2[1] * a2 * 0.95 + core[1] * hot * 0.9) * glow,
        (gas[2] * (a0 + a1 * 1.1) + gas2[2] * a2 * 0.95 + core[2] * hot * 0.9) * glow,
      ];
      // Emission is added to the dark behind it; the dust lanes block it.
      const k = (j * lw + i) * 4;
      img.data[k] = Math.min(255, col[0]);
      img.data[k + 1] = Math.min(255, col[1]);
      img.data[k + 2] = Math.min(255, col[2]);
      img.data[k + 3] = 255;
    }
  }
  const tile = makeCanvas(lw, lh);
  context(tile).putImageData(img, 0, 0);
  c.save();
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.globalCompositeOperation = "screen";
  c.drawImage(tile as unknown as CanvasImageSource, 0, 0, W, H);
  c.globalAlpha = 0.3;
  c.globalCompositeOperation = "lighter";
  c.drawImage(tile as unknown as CanvasImageSource, 0, 0, W, H);
  c.restore();
}

/** Bright stars with the six-pointed spikes of a segmented mirror. */
function paintSpikes(c: Ctx, f: Frame, r: () => number) {
  const { W, H, px } = f;
  const n = 4 + Math.floor(r() * 5);
  c.save();
  c.globalCompositeOperation = "lighter";
  for (let i = 0; i < n; i++) {
    const x = r() * W;
    const y = r() * H;
    const color = starColor(r);
    const size = (5 + r() * 9) * px;
    const len = size * (5 + r() * 5);
    const g = c.createRadialGradient(x, y, 0, x, y, size * 2.2);
    g.addColorStop(0, css([255, 255, 255], 0.95));
    g.addColorStop(0.3, css(color, 0.5));
    g.addColorStop(1, css(color, 0));
    c.fillStyle = g;
    c.fillRect(x - size * 2.2, y - size * 2.2, size * 4.4, size * 4.4);
    const spike = (angle: number, l: number, w: number) => {
      const dx = Math.cos(angle);
      const dy = Math.sin(angle);
      const s = c.createLinearGradient(x - dx * l, y - dy * l, x + dx * l, y + dy * l);
      s.addColorStop(0, css(color, 0));
      s.addColorStop(0.5, css(mix(color, [255, 255, 255], 0.5), 0.7));
      s.addColorStop(1, css(color, 0));
      c.strokeStyle = s;
      c.lineWidth = w;
      c.beginPath();
      c.moveTo(x - dx * l, y - dy * l);
      c.lineTo(x + dx * l, y + dy * l);
      c.stroke();
    };
    for (const a of [90, 30, 150]) spike(a * RAD, len, 1.1 * px);
    spike(0, len * 0.45, 0.8 * px);
  }
  c.restore();
}

/** A distant planet away from Ciel: a ringed giant or an ice world. */
function paintPlanet(c: Ctx, f: Frame, r: () => number) {
  const { W, H, spec } = f;
  const R = H * (0.075 + r() * 0.04);
  const left = spec.ciel ? spec.ciel.x > 0.5 : r() < 0.5;
  const x = (left ? 0.13 + r() * 0.08 : 0.79 + r() * 0.08) * W;
  const y = (0.2 + r() * 0.12) * H;
  const ringed = r() < 0.55;
  const tilt = (r() - 0.5) * 0.7;
  const lx = left ? 0.8 : -0.8;
  const ly = -0.45;
  const ringBands: [number, number][] = [[1.35, 0.25], [1.5, 0.55], [1.62, 0.45], [1.72, 0.08], [1.8, 0.4], [1.95, 0.28], [2.05, 0.12]];
  const ring = (front: boolean) => {
    c.save();
    c.translate(x, y);
    c.rotate(tilt);
    c.beginPath();
    if (front) c.rect(-R * 3, 0, R * 6, R * 3);
    else c.rect(-R * 3, -R * 3, R * 6, R * 3);
    c.clip();
    for (const [k, a] of ringBands) {
      c.strokeStyle = `rgba(226,206,170,${a})`;
      c.lineWidth = R * 0.11;
      c.beginPath();
      c.ellipse(0, 0, R * k, R * k * 0.24, 0, 0, Math.PI * 2);
      c.stroke();
    }
    c.restore();
  };
  if (ringed) ring(false);
  // Atmosphere: a glow around the limb, thicker on the side facing the light
  // (the body covers its inner part).
  const atm = ringed ? "255,214,160" : "140,220,255";
  const ax = x + lx * R * 0.12;
  const ay = y + ly * R * 0.12;
  const a = c.createRadialGradient(ax, ay, R * 0.9, ax, ay, R * 1.22);
  a.addColorStop(0, `rgba(${atm},0.42)`);
  a.addColorStop(1, `rgba(${atm},0)`);
  c.fillStyle = a;
  c.fillRect(ax - R * 1.3, ay - R * 1.3, R * 2.6, R * 2.6);
  c.save();
  c.translate(x, y);
  c.beginPath();
  c.arc(0, 0, R, 0, Math.PI * 2);
  c.clip();
  if (ringed) {
    c.rotate(tilt);
    const bands = ["#d9b98b", "#c79a6a", "#ead2a8", "#b98a5e", "#e2c497", "#cfa676", "#f0dcb8"];
    const n = 14;
    for (let i = 0; i < n; i++) {
      c.fillStyle = bands[Math.floor(r() * bands.length)];
      const by = -R + (2 * R * i) / n;
      c.fillRect(-R * 1.2, by, R * 2.4, (2 * R) / n + 1);
    }
    c.rotate(-tilt);
  } else {
    const g = c.createLinearGradient(0, -R, 0, R);
    g.addColorStop(0, "#9fe3ff");
    g.addColorStop(0.5, "#5aa8e8");
    g.addColorStop(1, "#3a6fc8");
    c.fillStyle = g;
    c.fillRect(-R, -R, R * 2, R * 2);
    for (let i = 0; i < 6; i++) {
      c.fillStyle = `rgba(255,255,255,${0.06 + r() * 0.08})`;
      c.fillRect(-R, -R + r() * 2 * R, R * 2, R * (0.03 + r() * 0.06));
    }
  }
  // Night side: a soft terminator across the disc, away from the light.
  const ll = Math.hypot(lx, ly);
  const sh = c.createLinearGradient((lx / ll) * R, (ly / ll) * R, (-lx / ll) * R, (-ly / ll) * R);
  sh.addColorStop(0, "rgba(0,0,8,0)");
  sh.addColorStop(0.42, "rgba(0,0,8,0.08)");
  sh.addColorStop(0.62, "rgba(0,0,8,0.7)");
  sh.addColorStop(1, "rgba(0,0,8,0.97)");
  c.fillStyle = sh;
  c.fillRect(-R, -R, R * 2, R * 2);
  c.restore();
  if (ringed) ring(true);
}

/** The average colour of a picture's lower half, from a tiny copy. */
export function averageColor(source: CanvasImageSource, w: number, h: number): RGB {
  const s = makeCanvas(8, 8);
  const c = context(s);
  c.drawImage(source, 0, h * 0.4, w, h * 0.6, 0, 0, 8, 8);
  const d = c.getImageData(0, 0, 8, 8).data;
  let r = 0;
  let g = 0;
  let b = 0;
  for (let i = 0; i < d.length; i += 4) {
    r += d[i];
    g += d[i + 1];
    b += d[i + 2];
  }
  const n = d.length / 4;
  return [r / n, g / n, b / n];
}

