// The sky renderer (src/sky/render.ts) on a recording fake canvas: every
// theme in every phase draws, with Ciel; the same seed and time give the same
// picture; frames reuse the static layers; a still becomes PNG bytes.

import { test } from "node:test";
import assert from "node:assert/strict";
import { FakeCanvas, installFakeCanvas } from "./fakecanvas.mjs";

installFakeCanvas();
let made = 0;
const canvases = [];
globalThis.OffscreenCanvas = class extends FakeCanvas {
  constructor(w, h) {
    super(w, h);
    made++;
    canvases.push(this);
  }
};

const { SkyRenderer, renderPng } = await import("../src/sky/render.ts");
const { sampleSky, sceneSpec } = await import("../src/sky/scenes.ts");
const { PHASES, THEMES, wallpaperPrefs } = await import("../src/sky/prefs.ts");

const W = 320;
const H = 134;
const spec = (theme, phase, mood = "idle", extra = {}) => {
  const s = sceneSpec(new Date(2024, 9, 18, 12), mood, wallpaperPrefs({}), {
    choice: { theme, phase }, sky: sampleSky(phase), ...extra,
  });
  s.seed = 42;
  return s;
};
const frame = (renderer, s, t, opts) => {
  const canvas = new FakeCanvas(W, H);
  const ctx = canvas.getContext("2d");
  renderer.render(ctx, s, t, opts);
  return ctx.calls;
};

test("a canvas that is still 0×0 draws nothing and doesn't throw", () => {
  const r = new SkyRenderer();
  for (const [w, h] of [[0, 0], [988, 0], [0, 414], [1, 1]]) {
    const ctx = new FakeCanvas(w, h).getContext("2d");
    assert.doesNotThrow(() => r.render(ctx, spec("sunset", "sunset"), 10));
    assert.equal(ctx.calls.length, 0);
  }
  // Once it has a size, the same renderer draws normally.
  assert.ok(frame(r, spec("sunset", "sunset"), 10).length > 0);
});

test("every theme draws in every phase, with Ciel on top", () => {
  for (const theme of THEMES) {
    for (const phase of PHASES) {
      const calls = frame(new SkyRenderer(), spec(theme, phase), 1000, { still: true });
      const images = calls.filter((c) => c[0] === "drawImage");
      assert.ok(images.length >= 2, `${theme}/${phase}: ${images.length} images`);
      // The static layers first, at full size; Ciel's sprite last of the images.
      assert.deepEqual(images[0].slice(1), [`canvas ${W}x${H}`, 0, 0], `${theme}/${phase}`);
    }
  }
});

test("the same seed and time give the same picture; time moves the clouds", () => {
  const s = spec("clear", "day");
  const a = frame(new SkyRenderer(), s, 500);
  const b = frame(new SkyRenderer(), s, 500);
  assert.deepEqual(a, b);
  const later = frame(new SkyRenderer(), s, 560);
  assert.notDeepEqual(a, later);
});

test("frames reuse the static layers; a new theme rebuilds them, a new mood doesn't", () => {
  const r = new SkyRenderer();
  const s = spec("night", "night");
  frame(r, s, 10);
  made = 0;
  for (let i = 1; i <= 24; i++) frame(r, s, 10 + i / 24);
  assert.equal(made, 0, "no canvas made while animating");
  frame(r, { ...s, mood: "sleeping" }, 12);
  assert.equal(made, 0, "Ciel's mood is drawn on the same layers");
  frame(r, spec("storm", "night"), 12);
  assert.ok(made > 0, "a new theme builds new layers");
});

test("the lock screen still and the live frame both draw; storms rain and flash", () => {
  const still = frame(new SkyRenderer(), spec("storm", "day", "error", { lockScreen: true }), 100, { still: true });
  assert.ok(still.some((c) => c[0] === "stroke"), "rain and the bolt are strokes");
  const live = frame(new SkyRenderer(), spec("storm", "day", "error"), 100);
  assert.ok(live.length > 0);
});

test("a photo replaces the drawn sky, cover-cropped", () => {
  const photo = new FakeCanvas(4000, 2000);
  canvases.length = 0;
  frame(new SkyRenderer(), spec("clear", "day"), 100, { photo, still: true });
  // The back layer holds the photo, scaled to cover 320×134 (2.39:1) from a
  // 2:1 picture: as wide as the screen, centred vertically, no sky under it.
  const back = canvases.find((c) => c.width === W && c.height === H);
  const calls = back.getContext("2d").calls;
  const draw = calls.find((c) => c[0] === "drawImage" && c[1] === "canvas 4000x2000");
  assert.ok(draw, JSON.stringify(calls));
  assert.deepEqual(draw.slice(2), [0, (H - 160) / 2, W, 160]);
  assert.equal(calls.filter((c) => c[0] === "fillRect").length, 0, "no sky painted");
});

test("a still becomes PNG bytes at the size asked", async () => {
  const bytes = await renderPng(spec("golden", "golden", "approval"), 640, 268, 1000);
  assert.ok(bytes instanceof Uint8Array);
  assert.equal(new TextDecoder().decode(bytes), "PNG 640x268");
});
