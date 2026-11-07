// The rain clock (Tools → Timers): a little cloud of Ciel's shape rains into
// a glass, and the glass is full when the time is up. Focus rounds rain
// indigo, breaks drizzle green; a timer that rang shakes its cloud. Drawn
// only while the Timers panel is on screen.

import { OUTLINE, OUTLINE_STEPS } from "../cloud/shape";

export type RainMood = "idle" | "timer" | "focus" | "break" | "paused" | "rang";

interface Drop {
  x: number;
  y: number;
  vy: number;
}

interface Ripple {
  x: number;
  age: number;
}

const W = 104;
const H = 100;
const GLASS = { x: 30, y: 52, w: 44, h: 44 };
const CLOUD = { x: 52, y: 21, r: 19 };

const COLORS: Record<RainMood, { cloud: string; water: string; drop: string }> = {
  idle: { cloud: "#3a3d44", water: "rgba(127,180,234,0.0)", drop: "#7fb4ea" },
  timer: { cloud: "#dfe7f3", water: "rgba(127,180,234,0.55)", drop: "#9cc7f2" },
  paused: { cloud: "#8a919c", water: "rgba(127,180,234,0.35)", drop: "#9cc7f2" },
  focus: { cloud: "#c7c9ff", water: "rgba(129,140,248,0.55)", drop: "#a5b4fc" },
  break: { cloud: "#c9f2dc", water: "rgba(52,211,153,0.5)", drop: "#6ee7b7" },
  rang: { cloud: "#ffd58a", water: "rgba(245,165,36,0.6)", drop: "#fbbf24" },
};

export class RainClock {
  readonly el: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null;
  private drops: Drop[] = [];
  private ripples: Ripple[] = [];
  private last = 0;
  private spawn = 0;
  private seed = 7;
  private level = 0;
  mood: RainMood = "idle";
  /** 0…1: how full the glass is. */
  progress = 0;

  constructor() {
    this.el = document.createElement("canvas");
    this.el.className = "rain-clock";
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    this.el.width = W * dpr;
    this.el.height = H * dpr;
    this.el.style.width = `${W}px`;
    this.el.style.height = `${H}px`;
    this.ctx = this.el.getContext("2d");
    this.ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  private rand(): number {
    this.seed = (this.seed * 16807) % 2147483647;
    return this.seed / 2147483647;
  }

  /** Steps and draws one frame. True while something moves. */
  frame(nowMs: number): boolean {
    const dt = this.last ? Math.min(0.05, (nowMs - this.last) / 1000) : 0;
    this.last = nowMs;
    const raining = this.mood === "timer" || this.mood === "focus" || this.mood === "break";
    // The water rises towards the progress, never jumping.
    this.level += (this.progress - this.level) * Math.min(1, dt * 4);
    const surface = GLASS.y + GLASS.h - 4 - (GLASS.h - 8) * this.level;
    if (raining) {
      this.spawn += dt * (this.mood === "break" ? 4 : 9);
      while (this.spawn >= 1) {
        this.spawn -= 1;
        this.drops.push({ x: CLOUD.x - 12 + this.rand() * 24, y: CLOUD.y + 14, vy: 30 + this.rand() * 20 });
      }
    }
    for (const d of this.drops) {
      d.vy += 260 * dt;
      d.y += d.vy * dt;
    }
    const landed = this.drops.filter((d) => d.y >= surface);
    for (const d of landed) this.ripples.push({ x: d.x, age: 0 });
    this.drops = this.drops.filter((d) => d.y < surface);
    for (const r of this.ripples) r.age += dt;
    this.ripples = this.ripples.filter((r) => r.age < 0.5);
    this.draw(nowMs, surface);
    return raining || this.mood === "rang" || this.drops.length > 0 || this.ripples.length > 0 || Math.abs(this.progress - this.level) > 0.002;
  }

  private draw(nowMs: number, surface: number) {
    const ctx = this.ctx;
    if (!ctx) return;
    const c = COLORS[this.mood];
    ctx.clearRect(0, 0, W, H);

    // The glass, and its water.
    ctx.save();
    roundRect(ctx, GLASS.x, GLASS.y, GLASS.w, GLASS.h, 9);
    ctx.clip();
    ctx.fillStyle = c.water;
    ctx.beginPath();
    const t = nowMs / 1000;
    ctx.moveTo(GLASS.x, surface);
    for (let x = 0; x <= GLASS.w; x += 4) {
      ctx.lineTo(GLASS.x + x, surface + Math.sin(t * 3 + x * 0.25) * 0.9);
    }
    ctx.lineTo(GLASS.x + GLASS.w, GLASS.y + GLASS.h);
    ctx.lineTo(GLASS.x, GLASS.y + GLASS.h);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
    ctx.strokeStyle = "rgba(255,255,255,0.22)";
    ctx.lineWidth = 1.5;
    roundRect(ctx, GLASS.x, GLASS.y, GLASS.w, GLASS.h, 9);
    ctx.stroke();

    // Ripples where the drops land.
    for (const r of this.ripples) {
      ctx.strokeStyle = `rgba(255,255,255,${0.35 * (1 - r.age / 0.5)})`;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.ellipse(r.x, surface, 2 + r.age * 10, 0.8 + r.age * 2, 0, 0, Math.PI * 2);
      ctx.stroke();
    }

    // The drops.
    ctx.strokeStyle = c.drop;
    ctx.lineWidth = 1.6;
    ctx.lineCap = "round";
    for (const d of this.drops) {
      ctx.beginPath();
      ctx.moveTo(d.x, d.y - 3.5);
      ctx.lineTo(d.x, d.y);
      ctx.stroke();
    }

    // The cloud: Ciel's own outline, a little face on it.
    const shake = this.mood === "rang" ? Math.sin(t * 40) * 1.6 : 0;
    const bob = this.mood === "idle" ? Math.sin(t * 1.2) * 0.8 : 0;
    const cx = CLOUD.x + shake;
    const cy = CLOUD.y + bob;
    ctx.fillStyle = c.cloud;
    ctx.beginPath();
    for (let i = 0; i <= OUTLINE_STEPS; i++) {
      const a = (i / OUTLINE_STEPS) * Math.PI * 2;
      const r = OUTLINE[i] * CLOUD.r;
      const x = cx + Math.cos(a) * r;
      const y = cy + Math.sin(a) * r;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = "#1b1d22";
    if (this.mood === "idle" || this.mood === "paused") {
      // Eyes shut: two little arcs.
      ctx.strokeStyle = "#1b1d22";
      ctx.lineWidth = 1.4;
      for (const dx of [-6, 6]) {
        ctx.beginPath();
        ctx.arc(cx + dx, cy - 1, 2.4, 0.15 * Math.PI, 0.85 * Math.PI);
        ctx.stroke();
      }
    } else {
      for (const dx of [-6, 6]) {
        ctx.beginPath();
        ctx.ellipse(cx + dx, cy - 1.5, 2, 2.6, 0, 0, Math.PI * 2);
        ctx.fill();
      }
      if (this.mood === "rang") {
        ctx.beginPath();
        ctx.ellipse(cx, cy + 5, 2.2, 2.6, 0, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + r);
  ctx.lineTo(x + w, y + h - r);
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  ctx.lineTo(x + r, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}
