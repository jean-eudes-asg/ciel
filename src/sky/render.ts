// renderScene(): a whole wallpaper at any size. The parts that don't move
// (the sky, stars, moon, sun, the high cloud deck, a nebula, the sea of clouds)
// are painted once into offscreen canvases and kept; a frame then only copies
// them and draws what moves: the drifting cumulus, twinkling stars, rain, a
// rare flash, and Ciel. Same seed and same `t`, same picture.

import { lightFor } from "./palette";
import {
  boltPath, buildCumulus, frameFor, lightningAt, paintCloudSea, paintCumulus, paintDeck, paintDither,
  paintLightning, paintMeteor, paintMilkyWay, paintMoon, paintRain, paintSky, paintSpace, paintStars,
  paintSun, paintTwinkle, averageColor, HORIZON, type Cumulus, type Frame, type Pt, type Star,
} from "./draw";
import { CielPainter } from "./ciel";
import { makeCanvas, context, toPng, type AnyCanvas } from "./canvas";
import type { SceneSpec } from "./scenes";

export interface RenderOptions {
  /** A picture that won't move (a still wallpaper, the lock screen): no blink, a bolt in a storm. */
  still?: boolean;
  /** A photo drawn instead of the sky (cover-cropped), with Ciel on top. */
  photo?: CanvasImageSource & { width: number; height: number } | null;
}

const photoIds = new WeakMap<object, number>();
let nextPhotoId = 1;
function photoKey(photo: RenderOptions["photo"]): number {
  if (!photo) return 0;
  let id = photoIds.get(photo);
  if (!id) {
    id = nextPhotoId++;
    photoIds.set(photo, id);
  }
  return id;
}

/** Keeps the static layers of the scene it drew last. One per canvas. */
export class SkyRenderer {
  private key = "";
  private frame: Frame | null = null;
  private back: AnyCanvas | null = null;
  private sea: { canvas: AnyCanvas; top: number } | null = null;
  private clouds: Cumulus[] = [];
  private twinklers: Star[] = [];
  private bolts = new Map<number, Pt[][]>();
  private ciel = new CielPainter();

  /** Drops the cached layers (they are rebuilt on the next frame). */
  reset() {
    this.key = "";
    this.back = null;
    this.sea = null;
    this.clouds = [];
    this.twinklers = [];
    this.bolts.clear();
  }

  render(ctx: CanvasRenderingContext2D, spec: SceneSpec, t: number, opts: RenderOptions = {}) {
    const W = ctx.canvas.width;
    const H = ctx.canvas.height;
    // A window still sizing itself (or hidden) has a 0×0 canvas: nothing to
    // draw, and building the layers at that size would throw.
    if (W < 2 || H < 2) return;
    const key = staticKey(spec, W, H, opts);
    if (key !== this.key) this.build(spec, W, H, opts);
    const f = this.frame!;
    // The scene's spec may change without the static layers (Ciel's mood).
    f.spec = spec;
    const still = opts.still ?? false;

    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = "source-over";
    ctx.drawImage(this.back as unknown as CanvasImageSource, 0, 0);
    if (!opts.photo) {
      paintTwinkle(ctx, this.twinklers, t, still);
      if (!still && (spec.theme === "night" || spec.theme === "space") && !spec.lockScreen) paintMeteor(ctx, f, t);
      paintCumulus(ctx, this.clouds, W, t);
      if (this.sea) ctx.drawImage(this.sea.canvas as unknown as CanvasImageSource, 0, this.sea.top);
      if (f.light.lightning) {
        const flash = still ? { k: 0.85, which: -1 } : lightningAt(spec.seed, t);
        if (flash.k > 0) {
          let bolt = this.bolts.get(flash.which);
          if (!bolt) {
            if (this.bolts.size > 8) this.bolts.clear();
            bolt = boltPath(f, flash.which);
            this.bolts.set(flash.which, bolt);
          }
          paintLightning(ctx, f, bolt, flash.k);
        }
      }
      paintRain(ctx, f, still ? 0 : t);
    }
    this.ciel.draw(ctx, f, t, still);
    ctx.restore();
  }

  private build(spec: SceneSpec, W: number, H: number, opts: RenderOptions) {
    this.reset();
    this.key = staticKey(spec, W, H, opts);
    const light = lightFor(spec.theme, spec.sun.elevation, spec.afterglow);
    const f = frameFor(spec, light, W, H);
    this.frame = f;
    const back = makeCanvas(W, H);
    const c = context(back);
    this.ciel.photoAmbient = null;
    if (opts.photo) {
      drawCover(c, opts.photo, W, H);
      this.ciel.photoAmbient = averageColor(opts.photo, opts.photo.width, opts.photo.height);
      light.ambientAmt = 0.25;
      light.rimAmt = 0.1;
      light.dark = 0;
    } else if (spec.theme === "space") {
      this.twinklers = paintSpace(c, f);
    } else {
      paintSky(c, f);
      this.twinklers = paintStars(c, f, 1, HORIZON);
      paintMilkyWay(c, f);
      paintMoon(c, f);
      paintSun(c, f);
      paintDeck(c, f);
      paintDither(c, f);
      this.sea = paintCloudSea(f);
      this.clouds = buildCumulus(f);
    }
    this.back = back;
  }
}

/** What the static layers depend on. Ciel's mood is not part of it. */
function staticKey(spec: SceneSpec, W: number, H: number, opts: RenderOptions): string {
  const r = (v: number, step: number) => Math.round(v / step) * step;
  const ciel = spec.ciel ? `${r(spec.ciel.x, 0.01)},${r(spec.ciel.y, 0.01)},${r(spec.ciel.size, 0.01)}` : "-";
  const moon = spec.moon ? `${r(spec.moon.elevation, 1)},${r(spec.moon.azimuth, 1)},${r(spec.moon.phase, 0.02)}` : "-";
  return [
    W, H, spec.theme, spec.afterglow, spec.seed, spec.facing, spec.lockScreen,
    r(spec.sun.elevation, 0.5), r(spec.sun.azimuth, 1), moon, ciel, photoKey(opts.photo),
  ].join("|");
}

/** A photo scaled to cover the whole screen, centred. */
function drawCover(c: CanvasRenderingContext2D, photo: CanvasImageSource & { width: number; height: number }, W: number, H: number) {
  const k = Math.max(W / photo.width, H / photo.height);
  const w = photo.width * k;
  const h = photo.height * k;
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.drawImage(photo, (W - w) / 2, (H - h) / 2, w, h);
}

const shared = new SkyRenderer();

/**
 * Draws the scene into `ctx` (its canvas's full size, in device pixels) at
 * `t` seconds. Reuses the static layers of the last call with the same scene.
 */
export function renderScene(ctx: CanvasRenderingContext2D, spec: SceneSpec, t: number, opts: RenderOptions = {}) {
  shared.render(ctx, spec, t, opts);
}

/** One still picture, encoded as PNG: a wallpaper or the lock screen. */
export async function renderPng(spec: SceneSpec, width: number, height: number, t: number, photo?: RenderOptions["photo"]): Promise<Uint8Array> {
  const canvas = makeCanvas(width, height);
  new SkyRenderer().render(context(canvas), spec, t, { still: true, photo: photo ?? null });
  return toPng(canvas);
}
