// The island's wallpaper controller (src/island/wallpaper.ts) on a fake clock:
// nothing at all while off; stills for every display and the lock screen;
// moods debounced, the lock screen at most every few minutes; a refresh every
// `refreshMinutes` and at a new phase of the day; live mode telling its
// windows; the session and "Apply now" forcing a redraw.

import { test } from "node:test";
import assert from "node:assert/strict";
import { TIMING, WallpaperController } from "../src/island/wallpaper.ts";
import { Bridge } from "../src/core/bridge.ts";
import { calls as ipc } from "./tauri.mjs";

const MIN = 60_000;
const START = new Date(2024, 9, 18, 12, 30);
/** Coordinates whose solar time matches this computer's clock (so 12:30 is day). */
const HERE = { lat: 48, lon: -START.getTimezoneOffset() / 4 };

function harness() {
  let now = START.getTime();
  let nextId = 1;
  const timers = new Map();
  const calls = [];
  let photos = [];
  let failing = false;
  const deps = {
    now: () => new Date(now),
    setTimeout: (fn, ms) => {
      const id = nextId++;
      timers.set(id, { at: now + ms, fn });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
    setInterval: (fn, ms) => {
      const id = nextId++;
      timers.set(id, { at: now + ms, fn, every: ms });
      return id;
    },
    clearInterval: (id) => timers.delete(id),
    monitors: async () => [
      { id: "A", x: 0, y: 0, width: 3440, height: 1440, scale: 1, primary: true },
      { id: "B", x: 3440, y: 0, width: 1920, height: 1080, scale: 1.25, primary: false },
    ],
    setDesktop: async (id) => {
      if (failing) throw new Error("no wallpaper today");
      calls.push(["desktop", id]);
    },
    setLockScreen: async () => calls.push(["lock"]),
    live: async (on) => calls.push(["live", on]),
    emitScene: (scene) => calls.push(["scene", scene.theme, scene.mood, scene.photoPath]),
    photos: async () => photos,
    render: async (spec, w, h, _t, photo) => {
      calls.push(["render", spec.theme, spec.mood, w, h, spec.lockScreen, photo]);
      return new Uint8Array(8);
    },
    log: (m) => calls.push(["log", m]),
  };
  const settle = async () => {
    for (let i = 0; i < 12; i++) await new Promise((r) => setImmediate(r));
  };
  /** Runs every timer due within `ms`, in order, letting the async work finish. */
  const advance = async (ms) => {
    const end = now + ms;
    for (;;) {
      let next = null;
      for (const [id, t] of timers) if (t.at <= end && (!next || t.at < next[1].at)) next = [id, t];
      if (!next) break;
      const [id, t] = next;
      now = t.at;
      if (t.every) t.at += t.every;
      else timers.delete(id);
      t.fn();
      await settle();
    }
    now = end;
    await settle();
  };
  const take = () => calls.splice(0);
  return {
    deps, calls, timers, advance, settle, take,
    setPhotos: (p) => (photos = p),
    fail: (f) => (failing = f),
  };
}

const renders = (calls) => calls.filter((c) => c[0] === "render");

test("nothing at all while the desktop and the lock screen are off", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  c.update({}, "idle");
  c.update({ live: true, cielSize: 0.3 }, "working");
  c.update({ live: true, cielSize: 0.3 }, "error");
  c.session(true);
  c.applyNow();
  c.liveReady();
  await h.advance(3 * 60 * MIN);
  assert.deepEqual(h.calls, []);
  assert.equal(h.timers.size, 0);
});

test("stills: one picture per display at its size, and the lock screen for the primary one", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  c.update({ desktop: true, lockScreen: true, live: false, ...HERE }, "idle");
  await h.advance(1000);
  const r = renders(h.calls);
  assert.deepEqual(r.map((x) => [x[3], x[4], x[5]]).sort(), [[1920, 1080, false], [3440, 1440, false], [3440, 1440, true]]);
  assert.ok(r.every((x) => x[1] === "clear" && x[2] === "idle"));
  assert.deepEqual(h.calls.filter((x) => x[0] !== "render").map((x) => x.join(":")).sort(), ["desktop:A", "desktop:B", "lock"]);
  assert.equal(h.calls.some((x) => x[0] === "live"), false);
});

test("a mood must settle before stills change; flickers in between draw nothing", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  const prefs = { desktop: true, live: false, ...HERE };
  c.update(prefs, "idle");
  await h.advance(5 * MIN);
  h.take();
  c.update(prefs, "working");
  await h.advance(5_000);
  c.update(prefs, "thinking");
  await h.advance(5_000);
  c.update(prefs, "idle");
  await h.advance(TIMING.stillDebounceMs + 1_000);
  assert.deepEqual(renders(h.take()), [], "back to what is shown: nothing to draw");
  c.update(prefs, "error");
  await h.advance(TIMING.stillDebounceMs - 1_000);
  assert.deepEqual(renders(h.take()), []);
  await h.advance(2_000);
  const r = renders(h.take());
  assert.equal(r.length, 2);
  assert.ok(r.every((x) => x[1] === "storm" && x[2] === "error"));
});

test("the lock screen changes at most every few minutes", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  const prefs = { lockScreen: true, ...HERE };
  c.update(prefs, "idle");
  await h.advance(1_000);
  assert.equal(renders(h.take()).length, 1);
  c.update(prefs, "error");
  await h.advance(TIMING.stillDebounceMs + 1_000);
  assert.deepEqual(renders(h.take()), [], "too soon after the last one");
  await h.advance(TIMING.lockMinGapMs - TIMING.stillDebounceMs);
  const r = renders(h.take());
  assert.equal(r.length, 1);
  assert.deepEqual([r[0][1], r[0][2], r[0][5]], ["storm", "error", true]);
});

test("unlocking brings the lock screen up to date at once", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  const prefs = { lockScreen: true, ...HERE };
  c.update(prefs, "idle");
  await h.advance(1_000);
  h.take();
  c.update(prefs, "finished");
  await h.advance(5_000);
  c.session(false);
  await h.settle();
  const r = renders(h.take());
  assert.equal(r.length, 1);
  assert.equal(r[0][2], "finished");
});

test("a fresh picture every refreshMinutes, and at a new phase of the day", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  c.update({ desktop: true, live: false, refreshMinutes: 15, ...HERE }, "idle");
  await h.advance(1_000);
  h.take();
  await h.advance(14 * MIN);
  assert.equal(renders(h.take()).length, 0);
  await h.advance(2 * MIN);
  assert.equal(renders(h.take()).length, 2, "both displays again after 15 minutes");

  // Rarely refreshed: only the phase changing draws, through the afternoon.
  const g = harness();
  const d = new WallpaperController(g.deps);
  d.update({ desktop: true, live: false, refreshMinutes: 24 * 60, ...HERE }, "idle");
  await g.advance(1_000);
  g.take();
  await g.advance(6 * 60 * MIN);
  const themes = renders(g.take()).map((x) => x[1]);
  assert.ok(themes.length >= 2 && themes.length <= 8, `drawn ${themes.length} times`);
  assert.ok(themes.includes("golden") || themes.includes("sunset"), themes.join(","));
});

test("live: the windows come up and hear the scene; the desktop gets no still", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  const prefs = { desktop: true, live: true, ...HERE };
  c.update(prefs, "idle");
  await h.advance(1_000);
  assert.deepEqual(h.take(), [["live", true], ["scene", "clear", "idle", null]]);
  c.update(prefs, "thinking");
  await h.advance(TIMING.liveDebounceMs - 200);
  assert.deepEqual(h.take(), []);
  await h.advance(400);
  assert.deepEqual(h.take(), [["scene", "cloudy", "thinking", null]]);
  // A window that reloads is told again.
  c.liveReady();
  await h.settle();
  assert.deepEqual(h.take(), [["scene", "cloudy", "thinking", null]]);
  await h.advance(60 * MIN);
  assert.equal(renders(h.take()).length, 0);
});

test("switching from live to still closes the windows and draws stills", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  c.update({ desktop: true, live: true, ...HERE }, "idle");
  await h.advance(1_000);
  h.take();
  c.update({ desktop: true, live: false, ...HERE }, "idle");
  await h.advance(1_000);
  const calls = h.take();
  assert.deepEqual(calls[0], ["live", false]);
  assert.equal(renders(calls).length, 2);
});

test("live where the desktop can't run it (GNOME, X11): the still picture stands in", async () => {
  const h = harness();
  h.deps.live = async (on) => {
    h.calls.push(["live", on]);
    return !on;
  };
  const c = new WallpaperController(h.deps);
  c.update({ desktop: true, live: true, ...HERE }, "idle");
  await h.advance(1_000);
  const calls = h.take();
  assert.deepEqual(calls[0], ["live", true]);
  assert.ok(calls.some((x) => x[0] === "desktop"), JSON.stringify(calls));
  // A new mood redraws the still, without asking for live windows again.
  c.update({ desktop: true, live: true, ...HERE }, "thinking");
  await h.advance(TIMING.stillMinGapMs + 1_000);
  const later = h.take();
  assert.equal(later.some((x) => x[0] === "live"), false);
  assert.ok(later.some((x) => x[0] === "desktop"), JSON.stringify(later));
});

test("turning everything off stops it all", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  c.update({ desktop: true, lockScreen: true, live: true, ...HERE }, "idle");
  await h.advance(1_000);
  h.take();
  c.update({ desktop: false, lockScreen: false, live: true, ...HERE }, "idle");
  assert.deepEqual(h.take(), [["live", false]]);
  assert.equal(h.timers.size, 0);
  await h.advance(60 * MIN);
  assert.deepEqual(h.calls, []);
});

test("Apply now redraws at once, even inside the gaps", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  c.update({ desktop: true, lockScreen: true, live: false, ...HERE }, "idle");
  await h.advance(1_000);
  h.take();
  c.applyNow();
  await h.settle();
  assert.equal(renders(h.take()).length, 3);
});

test("photos: a theme set to photos shows them in turn; none, the drawn sky", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  const prefs = { desktop: true, live: false, sources: { clear: "photos" }, ...HERE };
  c.update(prefs, "idle");
  await h.advance(1_000);
  assert.ok(renders(h.take()).every((x) => x[6] === null), "no photo yet");
  h.setPhotos([{ path: "p1" }, { path: "p2" }]);
  c.applyNow();
  await h.settle();
  const used = new Set(renders(h.take()).map((x) => x[6]));
  assert.equal(used.size, 1);
  assert.ok(used.has("p1") || used.has("p2"));
});

test("a failure is logged, never thrown, and tried again later", async () => {
  const h = harness();
  const c = new WallpaperController(h.deps);
  h.fail(true);
  c.update({ desktop: true, live: false, ...HERE }, "idle");
  await h.advance(1_000);
  assert.ok(h.take().some((x) => x[0] === "log"));
  h.fail(false);
  await h.advance(TIMING.stillMinGapMs + TIMING.tickMs);
  assert.ok(h.take().some((x) => x[0] === "desktop"));
});

test("a PNG goes to Rust as the raw body, the display's id percent-encoded in a header", async () => {
  const png = new Uint8Array([137, 80, 78, 71]);
  const id = "\\\\?\\DISPLAY#DEL40F6#5&2b2f&0&UID4352#{e6f07b5f}";
  await Bridge.wallpaperSetDesktop(id, png);
  const [cmd, body, options] = ipc.at(-1);
  assert.equal(cmd, "wallpaper_set_desktop");
  assert.equal(body, png);
  assert.equal(decodeURIComponent(options.headers.monitorId), id);
  assert.match(options.headers.monitorId, /^[A-Za-z0-9%._~!*'()-]+$/);
  await Bridge.wallpaperSetLockScreen(png);
  assert.equal(ipc.at(-1)[0], "wallpaper_set_lock_screen");
  assert.equal(ipc.at(-1)[1], png);
});
