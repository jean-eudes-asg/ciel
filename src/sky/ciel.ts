// Ciel in the wallpaper: the real character (BotEngine) in his mood, posed
// from the time alone so a frame is the same however it is reached. He
// floats and breathes, blinks now and then, and takes the scene's light: the
// sky's colour on his shade side, the sun's (or the moon's) on his rim, and a
// soft shadow on the clouds below him.

import { BOT_STATES, BotEngine } from "../cloud/engine";
import type { BotStateName } from "../core/layout";
import { hash01 } from "./noise";
import { css, mix, type RGB } from "./palette";
import { HORIZON, type Frame } from "./draw";
import { makeCanvas, context, type AnyCanvas } from "./canvas";

type Ctx = CanvasRenderingContext2D;

/** The engine's body is 0.684 × the width it is drawn at (radius 0.3 W × 1.14 × 2). */
const BODY_PER_BOX = 0.684;

/** How open his eyes are at `t`: a blink every few seconds, sometimes two. */
export function blinkAt(t: number, seed: number): number {
  const period = 4.3;
  const k = Math.floor(t / period);
  const start = k * period + hash01(seed, k) * (period - 0.7);
  const one = (d: number) => {
    if (d < 0 || d > 0.2) return 1;
    return d < 0.07 ? 1 - (d / 0.07) * 0.94 : 0.06 + ((d - 0.07) / 0.13) * 0.94;
  };
  let open = one(t - start);
  if (hash01(seed, k, 7) < 0.25) open = Math.min(open, one(t - start - 0.24));
  return open;
}

/** Sets every animated field of the engine for `mood` at `t`, without its clock. */
export function poseCiel(e: BotEngine, mood: BotStateName, t: number, still: boolean, seed = 0) {
  const cfg = BOT_STATES[mood];
  e.state = mood;
  e.cfg = cfg;
  e.col = cfg.color;
  e.colT = cfg.color;
  e.tint = cfg.tint;
  e.badge = cfg.badge;
  e.badgeS = cfg.badge ? 1 : 0;
  e.morph = 0;
  e.hands = 0;
  e.roll = 0;
  e.ox = 0;
  e.oy = 0;
  e.es = 1;
  e.blush = 0;
  e.particleOverhang = 0;
  let yaw = 0;
  let pitch = 0;
  if (cfg.look) {
    yaw = cfg.look[0] * 0.55;
    pitch = cfg.look[1] * 0.5;
  } else if (cfg.scans) {
    yaw = Math.sin(t * 1.4) * 0.55;
    pitch = -0.06;
  } else if (mood === "sleeping") {
    pitch = -0.14;
  } else if (mood === "dizzy") {
    yaw = Math.sin(t * 5) * 0.22;
  } else {
    // At rest he gazes around the sky, slowly.
    yaw = Math.sin(t * 0.21) * 0.2 + Math.sin(t * 0.077 + 1.3) * 0.14;
    pitch = -0.04 + Math.sin(t * 0.13) * 0.07;
  }
  e.yaw = yaw;
  e.pitch = pitch;
  e.tilt = cfg.tilt + (mood === "dizzy" ? Math.sin(t * 2.2) * 0.12 : Math.sin(t * 0.5) * 0.025);
  const breath = cfg.breathes ? 0.035 : 0.012;
  const speed = cfg.breathes ? 1.8 : 1.6;
  e.sy = 1 + Math.sin(t * speed) * breath;
  e.sx = 1 - Math.sin(t * speed) * breath * 0.57;
  if (cfg.bounces) e.oy = -Math.abs(Math.sin(t * 3.4)) * 0.06;
  e.open = still || mood === "sleeping" ? 1 : blinkAt(t, seed);
}

/** Draws Ciel into the scene. Keeps one engine and one sprite between frames. */
export class CielPainter {
  private engine = new BotEngine();
  private sprite: AnyCanvas | null = null;
  /** The colour of a photo behind him, for his shade side (photos only). */
  photoAmbient: RGB | null = null;

  draw(c: Ctx, f: Frame, t: number, still: boolean) {
    const place = f.spec.ciel;
    if (!place) return;
    const { W, H, light: l } = f;
    const body = place.size * H;
    const box = Math.max(8, Math.round(body / BODY_PER_BOX));
    const floatY = still ? 0 : Math.sin(t * 0.55) * 0.012 * H * Math.min(1, place.size / 0.2);
    const driftX = still ? 0 : Math.sin(t * 0.17) * 0.004 * H;
    const cx = place.x * W + driftX;
    const cy = place.y * H + floatY;
    const R = box * 0.3;

    this.shadow(c, f, cx, cy, body, R, floatY);

    // A faint glow around him after dark, so he never sinks into the night.
    if (l.dark > 0.3 || f.spec.theme === "space") {
      const g = c.createRadialGradient(cx, cy, R * 0.6, cx, cy, R * 2.6);
      const tone = mix(l.rim, [255, 255, 255], 0.5);
      g.addColorStop(0, css(tone, 0.2 * l.dark));
      g.addColorStop(1, css(tone, 0));
      c.fillStyle = g;
      c.fillRect(cx - R * 2.6, cy - R * 2.6, R * 5.2, R * 5.2);
    }

    const sw = box;
    const sh = Math.round(box * 1.0);
    if (!this.sprite || this.sprite.width !== sw || this.sprite.height !== sh) this.sprite = makeCanvas(sw, sh);
    const s = context(this.sprite);
    s.setTransform(1, 0, 0, 1, 0, 0);
    s.globalCompositeOperation = "source-over";
    s.clearRect(0, 0, sw, sh);
    poseCiel(this.engine, f.spec.mood, t, still, f.spec.seed);
    this.engine.draw(s, sw, sh);

    // The scene's light on him: its colour (warm at sunset), the sky's on his
    // shade side, the sun's on his rim.
    s.globalCompositeOperation = "source-atop";
    if (!this.photoAmbient) {
      const hi = Math.max(l.lit[0], l.lit[1], l.lit[2]);
      const lo = Math.min(l.lit[0], l.lit[1], l.lit[2]);
      const saturation = hi > 0 ? (hi - lo) / hi : 0;
      if (saturation > 0.04) {
        s.fillStyle = css(l.lit, Math.min(0.32, saturation * 0.45));
        s.fillRect(0, 0, sw, sh);
      }
    }
    const ambient = this.photoAmbient ?? l.ambient;
    const amb = s.createLinearGradient(0, sh, 0, sh * 0.25);
    amb.addColorStop(0, css(ambient, Math.min(0.6, l.ambientAmt * 1.1)));
    amb.addColorStop(1, css(ambient, 0));
    s.fillStyle = amb;
    s.fillRect(0, 0, sw, sh);
    if (l.dark > 0.05) {
      s.fillStyle = css(mix(ambient, [10, 14, 40], 0.5), (f.spec.theme === "space" ? 0.1 : 0.2) * l.dark);
      s.fillRect(0, 0, sw, sh);
    }
    if (l.rimAmt > 0.02) {
      let dx = f.lightAt.x - cx;
      let dy = f.lightAt.y - cy;
      const d = Math.hypot(dx, dy) || 1;
      dx /= d;
      dy /= d;
      const rx = sw / 2 + dx * R * 1.25;
      const ry = sh / 2 + dy * R * 1.0;
      const rim = s.createRadialGradient(rx, ry, 0, rx, ry, R * 1.2);
      rim.addColorStop(0, css(l.rim, l.rimAmt * f.lightAmt));
      rim.addColorStop(1, css(l.rim, 0));
      s.fillStyle = rim;
      s.fillRect(0, 0, sw, sh);
    }
    c.drawImage(this.sprite as unknown as CanvasImageSource, cx - sw / 2, cy - sh / 2);
    if (f.spec.mood === "sleeping") this.zzz(c, cx, cy, R, t, still);
  }

  /** A soft shadow: on the sea of clouds below him, else a halo of shade behind him. */
  private shadow(c: Ctx, f: Frame, cx: number, cy: number, body: number, R: number, floatY: number) {
    const { H, light: l } = f;
    const shade = mix(l.shade, [10, 16, 40], 0.5);
    const seaY = (HORIZON + 0.1) * H;
    if (f.spec.theme !== "space" && cy + R < seaY) {
      // Further below him, the shadow spreads and fades.
      const lift = (seaY - cy) / H;
      const spread = 1 + lift * 1.5 - floatY / H * 4;
      const dx = Math.max(-1, Math.min(1, (cx - f.lightAt.x) / H)) * body * 0.4;
      c.save();
      c.translate(cx + dx, seaY);
      c.scale(1, 0.3);
      const rr = body * 0.6 * spread;
      const g = c.createRadialGradient(0, 0, 0, 0, 0, rr);
      g.addColorStop(0, css(shade, 0.16 * (1 - l.dark * 0.5) / spread));
      g.addColorStop(0.5, css(shade, 0.08 * (1 - l.dark * 0.5) / spread));
      g.addColorStop(1, css(shade, 0));
      c.fillStyle = g;
      c.fillRect(-rr, -rr, rr * 2, rr * 2);
      c.restore();
    }
    // Contact shade just under his body, so he stands out from bright clouds.
    const g = c.createRadialGradient(cx, cy + R * 0.35, R * 0.3, cx, cy + R * 0.35, R * 1.6);
    g.addColorStop(0, css(shade, 0.16));
    g.addColorStop(1, css(shade, 0));
    c.fillStyle = g;
    c.fillRect(cx - R * 1.6, cy - R * 1.25, R * 3.2, R * 3.2);
  }

  /** Little z's rising while he sleeps. */
  private zzz(c: Ctx, cx: number, cy: number, R: number, t: number, still: boolean) {
    c.save();
    c.fillStyle = "rgb(214,224,240)";
    c.textAlign = "center";
    c.textBaseline = "middle";
    for (let i = 0; i < 3; i++) {
      const p = still ? 0.2 + i * 0.28 : (t * 0.28 + i / 3) % 1;
      const x = cx + R * (0.75 + p * 0.55) + Math.sin(p * 6 + i) * R * 0.08;
      const y = cy - R * (0.7 + p * 1.2);
      const size = R * (0.26 + p * 0.22);
      c.globalAlpha = Math.sin(p * Math.PI) * 0.85;
      c.font = `700 ${size}px system-ui, "Segoe UI", sans-serif`;
      c.fillText("z", x, y);
    }
    c.restore();
  }
}
