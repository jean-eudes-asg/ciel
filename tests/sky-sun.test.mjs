// The sun's math (src/sky/sun.ts): published sunrise and sunset times for a
// few cities and dates, the sun's position, the phase thresholds, the moon,
// and the place guessed from the time zone.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  PHASE_LIMITS, guessLocation, moonPhase, moonPosition, phaseOf, placeFor, solarPosition, sunTimes,
} from "../src/sky/sun.ts";

const MINUTE = 60_000;

/** Asserts `actual` is within `minutes` of the UTC time `expected` ("2024-06-21T03:47Z"). */
function near(actual, expected, minutes, what) {
  assert.ok(actual, `${what}: no time`);
  const diff = Math.abs(actual.getTime() - Date.parse(expected)) / MINUTE;
  assert.ok(diff <= minutes, `${what}: ${actual.toISOString()} is ${diff.toFixed(1)} min from ${expected}`);
}

// Times from timeanddate.com / NOAA's calculator, converted to UTC.
const CITIES = [
  { name: "Paris, summer solstice", lat: 48.8566, lon: 2.3522, at: "2024-06-21T12:00Z", rise: "2024-06-21T03:47Z", set: "2024-06-21T19:58Z" },
  { name: "New York, winter solstice", lat: 40.7128, lon: -74.006, at: "2024-12-21T17:00Z", rise: "2024-12-21T12:16Z", set: "2024-12-21T21:32Z" },
  { name: "Sydney, December", lat: -33.8688, lon: 151.2093, at: "2024-12-21T02:00Z", rise: "2024-12-20T18:41Z", set: "2024-12-21T09:05Z" },
  { name: "Tokyo, equinox", lat: 35.6762, lon: 139.6503, at: "2024-03-20T03:00Z", rise: "2024-03-19T20:45Z", set: "2024-03-20T08:53Z" },
  { name: "London, winter solstice", lat: 51.5074, lon: -0.1278, at: "2024-12-21T12:00Z", rise: "2024-12-21T08:04Z", set: "2024-12-21T15:54Z" },
];

for (const c of CITIES) {
  test(`sunrise and sunset within 3 minutes: ${c.name}`, () => {
    const times = sunTimes(new Date(c.at), c.lat, c.lon);
    assert.equal(times.polar, null);
    near(times.sunrise, c.rise, 3, "sunrise");
    near(times.sunset, c.set, 3, "sunset");
  });
}

test("the computer's time zone doesn't change the day computed", () => {
  // The solar day at the longitude decides, whatever the clock says.
  const a = sunTimes(new Date("2024-12-21T00:30Z"), -33.8688, 151.2093);
  const b = sunTimes(new Date("2024-12-21T08:00Z"), -33.8688, 151.2093);
  assert.equal(a.sunrise.getTime(), b.sunrise.getTime());
});

test("polar day and polar night", () => {
  const winter = sunTimes(new Date("2024-12-21T11:00Z"), 69.65, 18.96);
  assert.equal(winter.polar, "night");
  assert.equal(winter.sunrise, null);
  const summer = sunTimes(new Date("2024-06-21T11:00Z"), 69.65, 18.96);
  assert.equal(summer.polar, "day");
  assert.equal(summer.sunset, null);
});

test("the sun is high and due south at a northern noon, rises in the east", () => {
  const noon = solarPosition(new Date("2024-06-21T11:52Z"), 48.8566, 2.3522);
  assert.ok(Math.abs(noon.elevation - 64.6) < 0.5, `elevation ${noon.elevation}`);
  assert.ok(Math.abs(noon.azimuth - 180) < 3, `azimuth ${noon.azimuth}`);
  assert.ok(Math.abs(noon.hourAngle) < 1);
  const morning = solarPosition(new Date("2024-03-20T06:30Z"), 48.8566, 2.3522);
  assert.ok(morning.azimuth > 80 && morning.azimuth < 110, `azimuth ${morning.azimuth}`);
  assert.ok(morning.hourAngle < 0);
  // Southern hemisphere: the noon sun is in the north.
  const sydney = solarPosition(new Date("2024-06-21T01:57Z"), -33.8688, 151.2093);
  assert.ok(sydney.azimuth < 5 || sydney.azimuth > 355, `azimuth ${sydney.azimuth}`);
});

test("phase thresholds", () => {
  assert.equal(phaseOf(40, false), "day");
  assert.equal(phaseOf(PHASE_LIMITS.day, true), "day");
  assert.equal(phaseOf(PHASE_LIMITS.day - 0.1, false), "golden");
  assert.equal(phaseOf(PHASE_LIMITS.golden, true), "golden");
  assert.equal(phaseOf(PHASE_LIMITS.golden - 0.1, false), "sunset");
  assert.equal(phaseOf(PHASE_LIMITS.golden - 0.1, true), "dawn");
  assert.equal(phaseOf(PHASE_LIMITS.sunset, false), "sunset");
  assert.equal(phaseOf(PHASE_LIMITS.sunset - 0.1, false), "dusk");
  assert.equal(phaseOf(PHASE_LIMITS.night, false), "dusk");
  assert.equal(phaseOf(PHASE_LIMITS.night, true), "dawn");
  assert.equal(phaseOf(PHASE_LIMITS.night - 0.1, false), "night");
  assert.equal(phaseOf(-40, true), "night");
});

test("a day goes through every phase in order", () => {
  const seen = [];
  for (let m = 0; m < 24 * 60; m += 5) {
    const p = solarPosition(new Date(Date.parse("2024-09-22T00:00Z") + m * MINUTE), 48.8566, 2.3522);
    const phase = phaseOf(p.elevation, p.hourAngle < 0);
    if (seen.at(-1) !== phase) seen.push(phase);
  }
  assert.deepEqual(seen, ["night", "dawn", "golden", "day", "golden", "sunset", "dusk", "night"]);
});

test("the moon's phase: new and full moons of 2024", () => {
  const phase = (d) => moonPhase(new Date(d));
  const fromNew = (p) => Math.min(p, 1 - p);
  assert.ok(fromNew(phase("2024-10-02T18:49Z")) < 0.02);
  assert.ok(Math.abs(phase("2024-10-17T11:26Z") - 0.5) < 0.02);
  assert.ok(Math.abs(phase("2024-10-10T18:55Z") - 0.25) < 0.03);
});

test("a full moon is up at midnight and down at noon", () => {
  const midnight = moonPosition(new Date("2024-10-17T23:30Z"), 48.8566, 2.3522);
  assert.ok(midnight.elevation > 30, `midnight ${midnight.elevation}`);
  const noon = moonPosition(new Date("2024-10-17T11:30Z"), 48.8566, 2.3522);
  assert.ok(noon.elevation < 0, `noon ${noon.elevation}`);
});

test("a known time zone gives its city, an unknown one its offset and region", () => {
  assert.deepEqual(guessLocation("Europe/Paris", 60), { lat: 48.86, lon: 2.35 });
  assert.deepEqual(guessLocation("Australia/Sydney", 600), { lat: -33.87, lon: 151.21 });
  const unknown = guessLocation("Europe/Nowhere", 120);
  assert.equal(unknown.lon, 30);
  assert.equal(unknown.lat, 48);
  const south = guessLocation("America/Argentina/Salta", -180);
  assert.equal(south.lon, -45);
  assert.ok(south.lat < 0);
  const nothing = guessLocation("", 0);
  assert.equal(nothing.lon, 0);
  assert.equal(nothing.lat, 40);
});

test("the user's coordinates win over the guess", () => {
  const place = placeFor(12.5, -45.25);
  assert.equal(place.guessed, false);
  assert.equal(place.lat, 12.5);
  assert.equal(place.lon, -45.25);
  const guessed = placeFor(null, null);
  assert.equal(guessed.guessed, true);
  assert.ok(Math.abs(guessed.lat) <= 90 && Math.abs(guessed.lon) <= 180);
});
