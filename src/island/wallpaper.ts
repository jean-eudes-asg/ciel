// Ciel's wallpapers, island side. The island knows his mood, so it decides
// when the pictures change: a new phase of the day, a mood that has settled
// (the lock screen at most every few minutes), every `refreshMinutes`, and
// when the session locks or unlocks. It renders one PNG per display and hands
// it to Rust, or, in live mode, tells the live windows what to draw. The lock
// screen may show the screensaver's flip clock instead of the sky: drawn when
// the session locks, then every minute while it stays locked, never while it
// isn't. With the desktop and the lock screen both off it does nothing at
// all: no timer, no rendering, no call.

import { Bridge, onEvent, broadcast, type WallpaperMonitor, type WallpaperPhoto } from "../core/bridge";
import { State } from "../core/state";
import { HOST_OS } from "../core/pills";
import type { BotStateName } from "../core/layout";
import { wallpaperPrefs, type Theme, type WallpaperPrefs } from "../sky/prefs";
import { chooseScene, sceneSpec, secondsOfDay, type SceneSpec } from "../sky/scenes";
import { WALLPAPER_EVENTS, decodePhoto, type LiveScene } from "../sky/live";

export { WALLPAPER_EVENTS, type LiveScene } from "../sky/live";

export const TIMING = {
  /** How often the phase of the day and the refresh are checked while on. */
  tickMs: 60_000,
  /** A new mood reaches the live windows after this. */
  liveDebounceMs: 1_500,
  /** A mood must hold this long before still pictures are redrawn for it. */
  stillDebounceMs: 20_000,
  /** Desktop stills: at most this often, unless asked. */
  stillMinGapMs: 60_000,
  /** The lock screen: at most this often, unless asked. */
  lockMinGapMs: 3 * 60_000,
  /** Photos take turns, one per hour. */
  photoTurnMs: 3_600_000,
} as const;

/** Everything the controller reaches outside itself (the tests replace it). */
export interface WallpaperDeps {
  now(): Date;
  setTimeout(fn: () => void, ms: number): number;
  clearTimeout(id: number): void;
  setInterval(fn: () => void, ms: number): number;
  clearInterval(id: number): void;
  monitors(): Promise<WallpaperMonitor[] | null>;
  setDesktop(monitorId: string, png: Uint8Array): Promise<void>;
  setLockScreen(png: Uint8Array): Promise<void>;
  /** False when this desktop can't run live windows. */
  live(enabled: boolean): Promise<boolean>;
  emitScene(scene: LiveScene): void;
  photos(theme: Theme): Promise<WallpaperPhoto[] | null>;
  /** A PNG of `spec` at `width`×`height`; `photo` is a cached photo's path. */
  render(spec: SceneSpec, width: number, height: number, t: number, photo: string | null): Promise<Uint8Array>;
  /** A PNG of the screensaver's flip clock showing `now`, for the lock screen. */
  renderClock(width: number, height: number, now: Date): Promise<Uint8Array>;
  log(message: string): void;
}

type Target = "desktop" | "lock";

export class WallpaperController {
  private deps: WallpaperDeps;
  private prefs: WallpaperPrefs = wallpaperPrefs({});
  private prefsKey = "";
  private rawPrefs: unknown = undefined;
  private mood: BotStateName = "idle";
  private liveOn = false;
  /** Live mode as the settings ask for it; liveOn is what actually runs. */
  private liveWanted = false;
  private ticker: number | null = null;
  private timers = new Map<string, number>();
  /** What each still shows and when it was last drawn. */
  private shown: Record<Target, { key: string; at: number }> = {
    desktop: { key: "", at: -Infinity },
    lock: { key: "", at: -Infinity },
  };
  private liveKey = "";
  private running: Record<Target, boolean> = { desktop: false, lock: false };
  private again: Record<Target, boolean> = { desktop: false, lock: false };
  /** The session, as the last lock or unlock said. */
  private locked = false;

  constructor(deps: WallpaperDeps) {
    this.deps = deps;
  }

  get enabled(): boolean {
    return this.prefs.desktop || this.prefs.lockScreen;
  }

  /** The lock screen shows the flip clock, not the sky. */
  private get clockOnLock(): boolean {
    return this.prefs.lockScreen && this.prefs.lockStyle === "clock";
  }

  get isLive(): boolean {
    return this.liveOn;
  }

  /** Settings or mood may have changed. Cheap when nothing did (State notifies often). */
  update(raw: unknown, mood: BotStateName) {
    if (raw === this.rawPrefs && mood === this.mood) return;
    this.rawPrefs = raw;
    const prefs = wallpaperPrefs(raw);
    const key = JSON.stringify(prefs);
    const prefsChanged = key !== this.prefsKey;
    const moodChanged = mood !== this.mood;
    this.prefs = prefs;
    this.prefsKey = key;
    this.mood = mood;
    if (!this.enabled) {
      this.stop();
      return;
    }
    if (this.ticker == null) this.ticker = this.deps.setInterval(() => this.tick(), TIMING.tickMs);
    const wantLive = prefs.desktop && prefs.live;
    if (wantLive !== this.liveWanted) {
      this.liveWanted = wantLive;
      this.liveOn = wantLive;
      this.liveKey = "";
      void this.deps.live(wantLive).then((ok) => {
        // No live windows on this desktop: the still picture stands in.
        if (ok === false && wantLive && this.liveWanted) {
          this.deps.log("wallpaper: live unavailable, using a still picture");
          this.liveOn = false;
          this.refresh("desktop", true);
        }
      });
    }
    if (prefsChanged) {
      // New settings show at once (a slider saves when it is let go).
      this.later("prefs", 400, () => this.refreshAll(true));
      return;
    }
    if (moodChanged) {
      if (this.liveOn) this.later("live", TIMING.liveDebounceMs, () => void this.pushLive());
      if (prefs.desktop && !this.liveOn) this.later("desktop", TIMING.stillDebounceMs, () => this.refresh("desktop", false));
      if (prefs.lockScreen && !this.clockOnLock) this.later("lock", TIMING.stillDebounceMs, () => this.refresh("lock", false));
    }
  }

  /** "Apply now": everything, at once. */
  applyNow() {
    if (!this.enabled) return;
    this.refreshAll(true);
  }

  /**
   * The session was locked or unlocked: the lock screen catches up with the
   * mood, now. The flip clock is drawn as it locks, and not as it unlocks.
   */
  session(locked: boolean) {
    this.locked = locked;
    if (!this.prefs.lockScreen) return;
    if (this.clockOnLock) {
      if (locked) this.clockMinute();
      else this.cancel("clock");
      return;
    }
    this.refresh("lock", false, true);
  }

  /**
   * The flip clock as it is now, and again at each new minute while the
   * session stays locked. Rust says "locked" again at each minute too
   * (wallpaper/session.rs), for when a hidden page's timers are held back.
   */
  private clockMinute() {
    this.refresh("lock", false, true);
    const intoMinute = this.deps.now().getTime() % 60_000;
    this.later("clock", 60_000 - intoMinute + 500, () => {
      if (this.locked && this.clockOnLock) this.clockMinute();
    });
  }

  /** A live window (re)loaded: tell it what to draw. */
  liveReady() {
    if (!this.liveOn) return;
    this.liveKey = "";
    void this.pushLive();
  }

  /** Everything off: timers gone, live windows closed. */
  stop() {
    if (this.ticker != null) this.deps.clearInterval(this.ticker);
    this.ticker = null;
    for (const id of this.timers.values()) this.deps.clearTimeout(id);
    this.timers.clear();
    if (this.liveWanted) {
      this.liveWanted = false;
      this.liveOn = false;
      void this.deps.live(false);
    }
    this.liveKey = "";
    this.shown.desktop.key = "";
    this.shown.lock.key = "";
  }

  // ── Scheduling ──────────────────────────────────────────────────────────────

  private later(name: string, ms: number, fn: () => void) {
    this.cancel(name);
    this.timers.set(name, this.deps.setTimeout(() => {
      this.timers.delete(name);
      fn();
    }, ms));
  }

  private cancel(name: string) {
    const id = this.timers.get(name);
    if (id != null) this.deps.clearTimeout(id);
    this.timers.delete(name);
  }

  private refreshAll(force: boolean) {
    if (this.liveOn) {
      this.liveKey = "";
      void this.pushLive();
    }
    if (this.prefs.desktop && !this.liveOn) this.refresh("desktop", force);
    if (this.prefs.lockScreen) this.refresh("lock", force);
  }

  /** Every minute while on: a new phase or theme, or time for a fresh picture. */
  private tick() {
    if (!this.enabled) return;
    const now = this.deps.now().getTime();
    if (this.liveOn) void this.pushLive();
    const refresh = this.prefs.refreshMinutes * 60_000;
    for (const target of ["desktop", "lock"] as const) {
      if (target === "desktop" && (!this.prefs.desktop || this.liveOn)) continue;
      if (target === "lock" && !this.prefs.lockScreen) continue;
      // The flip clock keeps its own minutes, while locked (clockMinute).
      if (target === "lock" && this.clockOnLock) continue;
      const due = now - this.shown[target].at >= refresh;
      const changed = this.sceneKey() !== this.shown[target].key;
      if (due || changed) this.refresh(target, due);
    }
  }

  /**
   * Redraws a still if what it shows changed (or `force`), no sooner than its
   * minimum gap (unless `now`): a request inside the gap waits for its end.
   */
  private refresh(target: Target, force: boolean, now = false) {
    // Live mode draws the desktop itself: no still under it.
    if (target === "desktop" && (!this.prefs.desktop || this.liveOn)) return;
    if (target === "lock" && !this.prefs.lockScreen) return;
    if (!force && this.keyFor(target) === this.shown[target].key) return;
    const gap = target === "lock" ? TIMING.lockMinGapMs : TIMING.stillMinGapMs;
    const wait = this.shown[target].at + gap - this.deps.now().getTime();
    if (!force && !now && wait > 0) {
      if (!this.timers.has(`${target}-gap`)) {
        this.later(`${target}-gap`, wait, () => this.refresh(target, false));
      }
      return;
    }
    void this.draw(target);
  }

  // ── Drawing ─────────────────────────────────────────────────────────────────

  /** Theme, phase, mood and photo: what a picture shows. */
  private sceneKey(): string {
    const { theme, phase } = chooseScene(this.deps.now(), this.mood, this.prefs);
    // Photos take turns; a drawn sky doesn't change with the hour.
    const turn = this.prefs.sources[theme] === "photos" ? this.photoTurn() : "-";
    return `${theme}|${phase}|${this.mood}|${turn}|${this.prefsKey}`;
  }

  /** What a target's picture shows: the scene, or for the flip clock its minute. */
  private keyFor(target: Target): string {
    if (target === "lock" && this.clockOnLock) {
      return `clock|${Math.floor(this.deps.now().getTime() / 60_000)}|${this.prefsKey}`;
    }
    return this.sceneKey();
  }

  private photoTurn(): number {
    return Math.floor(this.deps.now().getTime() / TIMING.photoTurnMs);
  }

  /** The photo to show for a theme, if its source is photos and there are some. */
  private async photoFor(theme: Theme): Promise<string | null> {
    if (this.prefs.sources[theme] !== "photos") return null;
    const list = (await this.deps.photos(theme)) ?? [];
    if (list.length === 0) return null;
    return list[this.photoTurn() % list.length].path;
  }

  /** One run at a time per target; a request during a run makes one more after it. */
  private async draw(target: Target) {
    if (this.running[target]) {
      this.again[target] = true;
      return;
    }
    this.running[target] = true;
    try {
      const now = this.deps.now();
      const key = this.keyFor(target);
      const monitors = (await this.deps.monitors()) ?? [];
      if (target === "lock" && this.clockOnLock) {
        const primary = monitors.find((m) => m.primary) ?? monitors[0];
        await this.deps.setLockScreen(await this.deps.renderClock(primary?.width ?? 1920, primary?.height ?? 1080, now));
        this.shown[target] = { key, at: this.deps.now().getTime() };
        return;
      }
      const choice = chooseScene(now, this.mood, this.prefs);
      const photo = await this.photoFor(choice.theme);
      const t = secondsOfDay(now);
      if (target === "lock") {
        const primary = monitors.find((m) => m.primary) ?? monitors[0];
        const w = primary?.width ?? 1920;
        const h = primary?.height ?? 1080;
        const spec = sceneSpec(now, this.mood, this.prefs, { choice, lockScreen: true });
        await this.deps.setLockScreen(await this.deps.render(spec, w, h, t, photo));
      } else {
        const spec = sceneSpec(now, this.mood, this.prefs, { choice });
        for (const m of monitors) {
          await this.deps.setDesktop(m.id, await this.deps.render(spec, m.width, m.height, t, photo));
        }
      }
      this.shown[target] = { key, at: this.deps.now().getTime() };
    } catch (err) {
      this.deps.log(`wallpaper: the ${target} picture failed: ${String(err)}`);
      // Tried: wait for the next tick rather than retrying at once.
      this.shown[target].at = this.deps.now().getTime();
    } finally {
      this.running[target] = false;
      if (this.again[target]) {
        this.again[target] = false;
        this.refresh(target, false);
      }
    }
  }

  /** Tells the live windows what to draw, when it changed. */
  private async pushLive() {
    if (!this.liveOn) return;
    const choice = chooseScene(this.deps.now(), this.mood, this.prefs);
    const photoPath = await this.photoFor(choice.theme);
    const scene: LiveScene = {
      mood: this.mood,
      theme: choice.theme,
      phase: choice.phase,
      photoPath,
      lat: this.prefs.lat,
      lon: this.prefs.lon,
      ciel: { size: this.prefs.cielSize, x: this.prefs.cielX, y: this.prefs.cielY },
    };
    const key = JSON.stringify(scene);
    if (key === this.liveKey) return;
    this.liveKey = key;
    this.deps.emitScene(scene);
  }
}

// ── In the island ─────────────────────────────────────────────────────────────

/** The real world: Tauri, the clock, the renderer (loaded only once needed). */
export function islandDeps(): WallpaperDeps {
  return {
    now: () => new Date(),
    setTimeout: (fn, ms) => window.setTimeout(fn, ms),
    clearTimeout: (id) => window.clearTimeout(id),
    setInterval: (fn, ms) => window.setInterval(fn, ms),
    clearInterval: (id) => window.clearInterval(id),
    monitors: async () => {
      const list = await Bridge.wallpaperMonitors();
      // On Linux one picture covers every display: Rust applies the primary
      // one's only, so the others aren't drawn.
      if (!list || HOST_OS === "windows") return list;
      const primary = list.find((m) => m.primary) ?? list[0];
      return primary ? [primary] : [];
    },
    setDesktop: (id, png) => Bridge.wallpaperSetDesktop(id, png),
    setLockScreen: (png) => Bridge.wallpaperSetLockScreen(png),
    live: (enabled) => Bridge.wallpaperLive(enabled),
    emitScene: (scene) => void broadcast(WALLPAPER_EVENTS.scene, scene),
    photos: (theme) => Bridge.wallpaperPhotos(theme),
    render: async (spec, width, height, t, photo) => {
      const { renderPng } = await import("../sky/render");
      const bitmap = photo ? await decodePhoto(photo) : null;
      try {
        return await renderPng(spec, width, height, t, bitmap);
      } finally {
        bitmap?.close();
      }
    },
    renderClock: async (width, height, now) => {
      const { clockPng } = await import("../views/flipclock-picture");
      const s = State.settings;
      const lang = document.documentElement.lang || undefined;
      return clockPng(width, height, now, { format: s.clockFormat, size: s.screensaverSize, date: s.screensaverDate, lang });
    },
    log: (message) => void Bridge.log(message),
  };
}

/** Starts the wallpapers' controller in the island. */
export async function registerWallpaper() {
  const controller = new WallpaperController(islandDeps());
  const sync = () => controller.update(State.settings.wallpaper, State.effectiveState);
  State.subscribe(sync);
  sync();
  await onEvent<{ locked: boolean }>(WALLPAPER_EVENTS.session, (e) => controller.session(!!e?.locked));
  await onEvent<null>(WALLPAPER_EVENTS.ready, () => controller.liveReady());
  await onEvent<null>(WALLPAPER_EVENTS.apply, () => controller.applyNow());
  return controller;
}
