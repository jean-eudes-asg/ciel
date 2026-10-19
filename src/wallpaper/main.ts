// The live wallpaper (wallpaper.html): one window per display, behind the
// desktop icons (Rust places it). It draws the island's scene with src/sky,
// animated at 15 frames a second: the clouds drift, the stars twinkle, Ciel
// breathes and blinks in his mood. Paused (a fullscreen app, the battery, a
// locked session) or hidden, it draws nothing and keeps no timer.

import { IS_TAURI, emitToWindow, onEvent } from "../core/bridge";
import { wallpaperPrefs } from "../sky/prefs";
import { chooseScene, sceneSpec, secondsOfDay, type SceneSpec } from "../sky/scenes";
import { SkyRenderer } from "../sky/render";
import { WALLPAPER_EVENTS, decodePhoto, type LiveScene } from "../sky/live";
import { makeCanvas, context, type AnyCanvas } from "../sky/canvas";

/** 15 frames a second: the clouds drift slowly and Ciel breathes, so more buys nothing on screen. */
const FRAME_MS = 1000 / 15;
/** The sun moves: the sky is recomputed this often. */
const SKY_REFRESH_MS = 10 * 60_000;
/** A new sky fades in over this long. */
const FADE_MS = 1200;

class LiveWallpaper {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private renderer = new SkyRenderer();
  private scene: LiveScene | null = null;
  private spec: SceneSpec | null = null;
  private specAt = 0;
  private photo: ImageBitmap | null = null;
  private photoPath: string | null = null;
  private paused = false;
  private timer: number | null = null;
  private frame: number | null = null;
  private lastDraw = 0;
  /** The previous picture, fading out over the new one. */
  private fade: { canvas: AnyCanvas; start: number } | null = null;

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d", { alpha: false }) as CanvasRenderingContext2D;
    this.resize();
  }

  async start() {
    window.addEventListener("resize", () => this.resize());
    document.addEventListener("visibilitychange", () => (document.hidden ? this.stop() : this.schedule()));
    await onEvent<LiveScene>(WALLPAPER_EVENTS.scene, (s) => void this.onScene(s));
    // { paused, reason: "locked" | "displayOff" | "fullscreen" | "covered" | "battery" | null };
    // Rust also sends each window its state once the page has loaded.
    await onEvent<{ paused: boolean; reason: string | null }>(WALLPAPER_EVENTS.pause, (p) => this.setPaused(!!p?.paused));
    // The island may have spoken before this page listened: ask.
    await emitToWindow("island", WALLPAPER_EVENTS.ready);
    // In a plain browser (`npx vite`, /wallpaper.html?mood=…): the sky outside.
    if (!IS_TAURI) {
      const q = new URLSearchParams(location.search);
      const mood = (q.get("mood") ?? "idle") as LiveScene["mood"];
      const prefs = wallpaperPrefs({});
      const choice = chooseScene(new Date(), mood, prefs);
      void this.onScene({
        mood, theme: (q.get("theme") as LiveScene["theme"]) ?? choice.theme, phase: choice.phase,
        photoPath: null, lat: null, lon: null, ciel: { size: prefs.cielSize, x: prefs.cielX, y: prefs.cielY },
      });
    }
  }

  /** Device pixels: the window is the size of its display. */
  private resize() {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(window.innerWidth * dpr));
    const h = Math.max(1, Math.round(window.innerHeight * dpr));
    if (this.canvas.width === w && this.canvas.height === h) return;
    this.canvas.width = w;
    this.canvas.height = h;
    this.fade = null;
    this.lastDraw = 0;
    this.schedule();
  }

  private async onScene(scene: LiveScene) {
    const before = this.scene;
    this.scene = scene;
    if (scene.photoPath !== this.photoPath) {
      this.photoPath = scene.photoPath;
      const next = scene.photoPath ? await decodePhoto(scene.photoPath) : null;
      if (this.scene !== scene) {
        next?.close();
        return;
      }
      this.photo?.close();
      this.photo = next;
    }
    const sky = !before || before.theme !== scene.theme || before.phase !== scene.phase ||
      before.photoPath !== scene.photoPath || JSON.stringify(before.ciel) !== JSON.stringify(scene.ciel) ||
      before.lat !== scene.lat || before.lon !== scene.lon;
    this.rebuild(sky && !!before);
    this.schedule();
  }

  /** The spec from the scene and the clock; `fade` keeps the old picture to fade out. */
  private rebuild(fade: boolean) {
    const scene = this.scene;
    if (!scene) return;
    if (fade && this.lastDraw > 0) {
      const copy = makeCanvas(this.canvas.width, this.canvas.height);
      context(copy).drawImage(this.canvas, 0, 0);
      this.fade = { canvas: copy, start: performance.now() };
    }
    const prefs = wallpaperPrefs({
      lat: scene.lat, lon: scene.lon, cielSize: scene.ciel.size, cielX: scene.ciel.x, cielY: scene.ciel.y,
    });
    this.spec = sceneSpec(new Date(), scene.mood, prefs, { choice: { theme: scene.theme, phase: scene.phase } });
    this.specAt = performance.now();
  }

  private setPaused(paused: boolean) {
    this.paused = paused;
    if (paused) this.stop();
    else this.schedule();
  }

  // ── Frames ──────────────────────────────────────────────────────────────────

  private get running(): boolean {
    return !this.paused && !document.hidden && this.scene != null;
  }

  private schedule() {
    if (!this.running || this.timer != null || this.frame != null) return;
    const wait = Math.max(0, this.lastDraw + FRAME_MS - performance.now());
    this.timer = window.setTimeout(() => {
      this.timer = null;
      this.frame = requestAnimationFrame((now) => {
        this.frame = null;
        this.draw(now);
        this.schedule();
      });
    }, wait);
  }

  /** Nothing left pending: no timer, no frame. */
  private stop() {
    if (this.timer != null) window.clearTimeout(this.timer);
    if (this.frame != null) cancelAnimationFrame(this.frame);
    this.timer = null;
    this.frame = null;
  }

  private draw(now: number) {
    if (!this.running || !this.spec) return;
    this.lastDraw = now;
    if (now - this.specAt > SKY_REFRESH_MS) this.rebuild(true);
    const t = secondsOfDay(new Date());
    this.renderer.render(this.ctx, this.spec!, t, { photo: this.photo });
    if (this.fade) {
      const p = (now - this.fade.start) / FADE_MS;
      if (p >= 1) {
        this.fade = null;
      } else {
        this.ctx.save();
        this.ctx.globalAlpha = 1 - p * p * (3 - 2 * p);
        this.ctx.drawImage(this.fade.canvas as unknown as CanvasImageSource, 0, 0);
        this.ctx.restore();
      }
    }
  }
}

const canvas = document.getElementById("sky") as HTMLCanvasElement | null;
if (canvas) void new LiveWallpaper(canvas).start();
