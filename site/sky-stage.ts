// The sky behind the whole page: the app's own SkyRenderer, one per scene,
// drawn into one canvas. Between two scenes the next one is drawn into a
// scratch canvas and laid over the first, as much as the scroll says (k), so
// skies crossfade smoothly. Building a scene's still layers takes a few
// hundred milliseconds, so the stage normally runs in a worker
// (sky-worker.ts) on an OffscreenCanvas and builds every scene ahead of time
// there, away from scrolling and from Ciel; where that isn't possible it runs
// on the page itself and builds each scene only when it is needed.

import { SkyRenderer } from "../src/sky/render";
import type { SceneSpec } from "../src/sky/scenes";
import { context, makeCanvas, type AnyCanvas } from "../src/sky/canvas";

/** What the page tells the stage. */
export type StageMessage =
  | {
      type: "init";
      canvas?: OffscreenCanvas;
      width: number;
      height: number;
      scenes: Record<string, SceneSpec>;
      /** Scenes in the order the page shows them: the order they are built in. */
      order: string[];
      view: StageView;
      /** Reduced motion: a still sky, drawn only when something changes. */
      still: boolean;
      /** Seconds since local midnight: where the clouds have drifted to. */
      t: number;
      /** Build every scene ahead of time (in a worker, where it doesn't block anything). */
      prebuild: boolean;
    }
  | { type: "size"; width: number; height: number }
  | { type: "scenes"; scenes: Record<string, SceneSpec> }
  | { type: "view"; view: StageView }
  | { type: "run"; running: boolean };

/** Scene `a`, with scene `b` over it at opacity `k`. */
export interface StageView {
  a: string;
  b: string;
  k: number;
}

/** What the stage tells the page. */
export type StageEvent = { type: "drawn" };

const FPS = 30;

export class SkyStage {
  private ctx: CanvasRenderingContext2D;
  private scratch: AnyCanvas | null = null;
  private renderers = new Map<string, SkyRenderer>();
  /** Scenes whose still layers exist at the current size. */
  private built = new Set<string>();
  private scenes: Record<string, SceneSpec>;
  private order: string[];
  private view: StageView;
  private still: boolean;
  private prebuild: boolean;
  private running = false;
  private dirty = true;
  private last = 0;
  private start = performance.now();
  private t0: number;
  private announced = false;

  constructor(private canvas: AnyCanvas, init: Extract<StageMessage, { type: "init" }>, private emit: (e: StageEvent) => void) {
    canvas.width = Math.max(2, init.width);
    canvas.height = Math.max(2, init.height);
    this.ctx = context(canvas);
    this.scenes = init.scenes;
    this.order = init.order;
    this.view = init.view;
    this.still = init.still;
    this.prebuild = init.prebuild;
    this.t0 = init.t;
  }

  handle(msg: StageMessage) {
    switch (msg.type) {
      case "size":
        if (msg.width === this.canvas.width && msg.height === this.canvas.height) return;
        this.canvas.width = Math.max(2, msg.width);
        this.canvas.height = Math.max(2, msg.height);
        // Every scene's layers were for the old size: let them go and build again.
        this.renderers.clear();
        this.built.clear();
        this.scratch = null;
        this.dirty = true;
        break;
      case "scenes":
        for (const [name, spec] of Object.entries(msg.scenes)) {
          if (JSON.stringify(spec) === JSON.stringify(this.scenes[name])) continue;
          this.scenes[name] = spec;
          this.built.delete(name);
        }
        this.dirty = true;
        break;
      case "view":
        if (msg.view.a === this.view.a && msg.view.b === this.view.b && Math.abs(msg.view.k - this.view.k) < 0.002) return;
        this.view = msg.view;
        this.dirty = true;
        break;
      case "run":
        this.running = msg.running;
        break;
      case "init":
        return;
    }
    this.schedule();
  }

  private renderer(name: string): SkyRenderer {
    let r = this.renderers.get(name);
    if (!r) {
      r = new SkyRenderer();
      this.renderers.set(name, r);
    }
    return r;
  }

  private scratchCtx(): CanvasRenderingContext2D {
    const { width, height } = this.canvas;
    if (!this.scratch || this.scratch.width !== width || this.scratch.height !== height) {
      this.scratch = makeCanvas(width, height);
    }
    return context(this.scratch);
  }

  private t(): number {
    return this.t0 + (performance.now() - this.start) / 1000;
  }

  /** Draws scene `name` into `ctx` (building its layers first if needed). */
  private paint(ctx: CanvasRenderingContext2D, name: string, t: number) {
    const spec = this.scenes[name];
    if (!spec) return;
    this.renderer(name).render(ctx, spec, t, { still: this.still });
    this.built.add(name);
  }

  draw() {
    const { a, b, k } = this.view;
    const t = this.still ? this.t0 : this.t();
    if (k >= 0.999 || a === b) {
      this.paint(this.ctx, k >= 0.999 ? b : a, t);
    } else {
      this.paint(this.ctx, a, t);
      if (k > 0.001) {
        const s = this.scratchCtx();
        this.paint(s, b, t);
        this.ctx.save();
        this.ctx.globalAlpha = k;
        this.ctx.drawImage(this.scratch as unknown as CanvasImageSource, 0, 0);
        this.ctx.restore();
      }
    }
    this.dirty = false;
    if (!this.announced) {
      this.announced = true;
      this.emit({ type: "drawn" });
    }
  }

  /** Builds the nearest scene not built yet. False when there is none. */
  private buildOne(): boolean {
    const here = Math.max(0, this.order.indexOf(this.view.k >= 0.5 ? this.view.b : this.view.a));
    const todo = this.order
      .map((name, i) => ({ name, d: i >= here ? i - here : (here - i) * 1.5 }))
      .filter((s) => !this.built.has(s.name) && this.scenes[s.name])
      .sort((x, y) => x.d - y.d);
    if (!todo.length) return false;
    this.paint(this.scratchCtx(), todo[0].name, this.t());
    return true;
  }

  private tick = () => {
    this.queued = false;
    const now = performance.now();
    const animate = this.running && !this.still;
    if (this.dirty || (animate && now - this.last >= 1000 / FPS - 4)) {
      this.last = now;
      this.draw();
    } else if (this.prebuild && this.running) {
      // One scene per turn, between two frames.
      this.buildOne();
    }
    this.schedule();
  };

  private queued = false;

  private schedule() {
    if (this.queued) return;
    const animate = this.running && !this.still;
    const pending = this.dirty || (this.prebuild && this.running && this.built.size < this.order.length);
    if (!animate && !pending) return;
    this.queued = true;
    // In step with the display where frames exist (workers have them in most browsers).
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(this.tick);
    else setTimeout(this.tick, 16);
  }
}
