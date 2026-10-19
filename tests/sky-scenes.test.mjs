// What the wallpaper shows (src/sky/scenes.ts, prefs.ts): the time of day
// picks the light, the mood picks the weather, the user's mapping wins; the
// sun is moved to suit a theme; Ciel keeps clear of the lock screen's clock;
// broken preferences fall back to their defaults.

import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_WALLPAPER, MOODS, PHASES, THEMES, wallpaperPrefs } from "../src/sky/prefs.ts";
import {
  chooseScene, cielPlacement, defaultTheme, drawnMoon, drawnSun, sampleSky, sceneSpec, seedFor,
} from "../src/sky/scenes.ts";

/** A place whose solar time matches this computer's clock, so local hours mean what they say. */
const local = (date) => ({ lat: 48, lon: -date.getTimezoneOffset() / 4, guessed: false, zone: "" });
const at = (h, m = 0) => new Date(2024, 9, 18, h, m);

test("idle and working follow the time of day", () => {
  const expected = { day: "clear", golden: "golden", dawn: "golden", sunset: "sunset", dusk: "sunset", night: "night" };
  for (const mood of ["idle", "working"]) {
    for (const phase of PHASES) assert.equal(defaultTheme(mood, phase, 12), expected[phase], `${mood} at ${phase}`);
  }
});

test("the other moods pick their weather whatever the hour", () => {
  for (const phase of PHASES) {
    for (const mood of ["thinking", "searching", "ratelimit"]) assert.equal(defaultTheme(mood, phase, 12), "cloudy");
    for (const mood of ["approval", "question"]) assert.equal(defaultTheme(mood, phase, 12), "golden");
    assert.equal(defaultTheme("error", phase, 12), "storm");
    assert.equal(defaultTheme("finished", phase, 12), "sunset");
    assert.equal(defaultTheme("dizzy", phase, 12), "space");
  }
});

test("sleeping: a night sky, or space late at night", () => {
  assert.equal(defaultTheme("sleeping", "day", 14), "night");
  assert.equal(defaultTheme("sleeping", "night", 22), "night");
  assert.equal(defaultTheme("sleeping", "night", 2), "space");
  assert.equal(defaultTheme("sleeping", "dawn", 4), "night");
});

test("chooseScene reads the phase from the real sun", () => {
  const prefs = wallpaperPrefs({});
  const noon = at(12, 30);
  assert.deepEqual(chooseScene(noon, "idle", prefs, local(noon)), { theme: "clear", phase: "day" });
  const night = at(1);
  assert.deepEqual(chooseScene(night, "idle", prefs, local(night)), { theme: "night", phase: "night" });
  assert.deepEqual(chooseScene(night, "sleeping", prefs, local(night)), { theme: "space", phase: "night" });
  assert.deepEqual(chooseScene(noon, "error", prefs, local(noon)), { theme: "storm", phase: "day" });
});

test("a mood mapped to a theme wins; Auto and junk don't", () => {
  const noon = at(12, 30);
  const prefs = wallpaperPrefs({ moods: { idle: "space", working: "auto", error: "lava", thinking: "golden" } });
  assert.equal(chooseScene(noon, "idle", prefs, local(noon)).theme, "space");
  assert.equal(chooseScene(noon, "working", prefs, local(noon)).theme, "clear");
  assert.equal(chooseScene(noon, "error", prefs, local(noon)).theme, "storm");
  assert.equal(chooseScene(noon, "thinking", prefs, local(noon)).theme, "golden");
  assert.equal(prefs.moods.error, undefined, "an unknown theme is dropped");
});

test("the sun is moved to suit a theme, and kept where it is when it fits", () => {
  const day = sampleSky("day");
  assert.equal(drawnSun("clear", day).elevation, day.sun.elevation);
  const golden = drawnSun("golden", day);
  assert.equal(golden.elevation, 6);
  assert.equal(golden.afterglow, false);
  assert.ok(Math.abs(golden.azimuth - day.sunsetAzimuth) < 1e-9 || Math.abs(golden.azimuth - day.sunriseAzimuth) < 1e-9);
  const sunset = drawnSun("sunset", sampleSky("sunset"));
  assert.equal(sunset.elevation, sampleSky("sunset").sun.elevation);
  // A sunset after dark: a night sky with a warm horizon.
  const late = drawnSun("sunset", sampleSky("night"));
  assert.equal(late.afterglow, true);
  assert.ok(late.elevation < -10);
  // A fine day while it's dark out: the sun is put back in the sky.
  assert.ok(drawnSun("clear", sampleSky("night")).elevation > 20);
  assert.ok(drawnSun("night", day).elevation <= -22);
});

test("the moon shows at night, never in a storm or in space", () => {
  const night = sampleSky("night");
  assert.ok(drawnMoon("night", night, -30));
  assert.equal(drawnMoon("storm", night, -30), null);
  assert.equal(drawnMoon("space", night, -30), null);
  assert.equal(drawnMoon("clear", sampleSky("day"), 38), null);
  // Asleep at noon: a night sky with at least a crescent.
  const moon = drawnMoon("night", sampleSky("day"), -22);
  assert.ok(moon && moon.elevation > 0 && moon.phase > 0.05 && moon.phase < 0.95);
});

test("on the lock screen Ciel stays below the clock", () => {
  const prefs = wallpaperPrefs({ cielY: 0.3, cielSize: 0.35, cielX: 0.2 });
  assert.deepEqual(cielPlacement(prefs, false), { size: 0.35, x: 0.2, y: 0.3 });
  const lock = cielPlacement(prefs, true);
  assert.ok(lock.y >= 0.7);
  assert.ok(lock.size <= 0.2);
  assert.equal(lock.x, 0.2);
});

test("sceneSpec puts it together, a new seed each day", () => {
  const noon = at(12, 30);
  const spec = sceneSpec(noon, "finished", wallpaperPrefs({}), { place: local(noon) });
  assert.equal(spec.theme, "sunset");
  assert.equal(spec.mood, "finished");
  assert.equal(spec.lockScreen, false);
  assert.ok(spec.ciel);
  assert.equal(sceneSpec(noon, "idle", wallpaperPrefs({}), { noCiel: true, place: local(noon) }).ciel, null);
  assert.equal(seedFor(at(9)), seedFor(at(23)));
  assert.notEqual(seedFor(at(9)), seedFor(new Date(2024, 9, 19, 9)));
});

test("preferences: off by default, broken fields put back", () => {
  assert.equal(DEFAULT_WALLPAPER.desktop, false);
  assert.equal(DEFAULT_WALLPAPER.lockScreen, false);
  const p = wallpaperPrefs({
    desktop: "yes", live: 0, lat: 123, lon: -45, cielSize: 9, cielX: -1, refreshMinutes: "soon",
    sources: { night: "photos", storm: "paint", lava: "photos" }, extra: { kept: true },
  });
  assert.equal(p.desktop, false);
  assert.equal(p.live, true);
  assert.equal(p.lat, null);
  assert.equal(p.lon, -45);
  assert.equal(p.cielSize, 0.6);
  assert.equal(p.cielX, 0);
  assert.equal(p.refreshMinutes, 15);
  assert.deepEqual(p.sources, { night: "photos" });
  assert.deepEqual(p.extra, { kept: true }, "unknown keys are kept");
  assert.deepEqual(wallpaperPrefs(null), wallpaperPrefs({}));
  assert.equal(THEMES.length, 7);
  assert.equal(MOODS.length, 11);
});
