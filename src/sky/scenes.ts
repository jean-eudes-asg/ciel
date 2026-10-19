// What to draw: the time of day picks the light, Ciel's mood picks the
// weather. chooseScene() is the decision; sceneSpec() turns it into everything
// the renderer needs (where the sun and the moon are drawn, the seed, where
// Ciel floats).

import type { BotStateName } from "../core/layout";
import { isTheme, type Phase, type Theme, type WallpaperPrefs } from "./prefs";
import {
  moonPosition, phaseOf, placeFor, solarPosition, sunTimes, wrap180, type Place, type SunPosition,
} from "./sun";
import { hash } from "./noise";

export interface SceneChoice {
  theme: Theme;
  phase: Phase;
}

/** Local hours when a sleeping Ciel drifts off into space rather than a night sky. */
const LATE_NIGHT = [0, 5] as const;

/** The theme a mood gets when the user left it on "Auto". */
export function defaultTheme(mood: BotStateName, phase: Phase, localHour: number): Theme {
  switch (mood) {
    case "thinking":
    case "searching":
    case "ratelimit":
      return "cloudy";
    case "approval":
    case "question":
      return "golden";
    case "error":
      return "storm";
    case "finished":
      return "sunset";
    case "sleeping":
      return phase === "night" && localHour >= LATE_NIGHT[0] && localHour < LATE_NIGHT[1] ? "space" : "night";
    case "dizzy":
      return "space";
    default:
      // idle, working: the sky outside.
      switch (phase) {
        case "day": return "clear";
        case "golden": return "golden";
        case "dawn": return "golden";
        case "sunset": return "sunset";
        case "dusk": return "sunset";
        default: return "night";
      }
  }
}

/** The theme and phase for `mood` at `now`: a mood the user mapped to a theme wins. */
export function chooseScene(now: Date, mood: BotStateName, prefs: WallpaperPrefs, place?: Place): SceneChoice {
  const at = place ?? placeFor(prefs.lat, prefs.lon, now);
  const sun = solarPosition(now, at.lat, at.lon);
  const phase = phaseOf(sun.elevation, sun.hourAngle < 0);
  const picked = prefs.moods[mood];
  const theme = isTheme(picked) ? picked : defaultTheme(mood, phase, now.getHours());
  return { theme, phase };
}

// ── The real sky ──────────────────────────────────────────────────────────────

export interface RealSky {
  sun: SunPosition;
  moon: SunPosition & { phase: number };
  /** Where the sun rises and sets today (degrees from north). */
  sunriseAzimuth: number;
  sunsetAzimuth: number;
  /** The viewer faces the equator: south in the northern hemisphere. */
  facing: "south" | "north";
}

export function realSky(now: Date, place: Place): RealSky {
  const sun = solarPosition(now, place.lat, place.lon);
  const moon = moonPosition(now, place.lat, place.lon);
  const times = sunTimes(now, place.lat, place.lon);
  const facing = place.lat >= 0 ? "south" : "north";
  // Polar day or night: east and west will do.
  const az = (d: Date | null, fallback: number) => (d ? solarPosition(d, place.lat, place.lon).azimuth : fallback);
  return {
    sun, moon,
    sunriseAzimuth: az(times.sunrise, 90),
    sunsetAzimuth: az(times.sunset, 270),
    facing,
  };
}

/** A believable sky for a phase, for previews (Paris, an autumn day). */
export function sampleSky(phase: Phase): RealSky {
  const sun: Record<Phase, [number, number, number]> = {
    night: [-32, 330, 150],
    dawn: [-3, 100, -95],
    day: [38, 175, -5],
    golden: [6, 238, 62],
    sunset: [0.4, 252, 82],
    dusk: [-6.5, 262, 95],
  };
  const [elevation, azimuth, hourAngle] = sun[phase];
  return {
    sun: { elevation, azimuth, hourAngle },
    moon: { elevation: 34, azimuth: 140, hourAngle: -35, phase: 0.36 },
    sunriseAzimuth: 105,
    sunsetAzimuth: 255,
    facing: "south",
  };
}

// ── The spec the renderer draws ───────────────────────────────────────────────

export interface CielPlacement {
  /** Width of his body, as a fraction of the screen height. */
  size: number;
  /** His centre, as fractions of the width and the height. */
  x: number;
  y: number;
}

export interface SceneSpec {
  theme: Theme;
  phase: Phase;
  mood: BotStateName;
  /** The sun as drawn (degrees): the real one, or moved to suit the theme. */
  sun: { elevation: number; azimuth: number };
  /** The moon as drawn, null when it isn't in the picture. */
  moon: { elevation: number; azimuth: number; phase: number } | null;
  facing: "south" | "north";
  /** A sunset or golden theme after dark: a night sky with a warm horizon. */
  afterglow: boolean;
  /** Fixes the clouds and the stars. */
  seed: number;
  /** Null: the scene without Ciel. */
  ciel: CielPlacement | null;
  /** The lock screen: room left for the clock, nothing that moves. */
  lockScreen: boolean;
}

/** Where the sun is drawn for a theme, given the real one. */
export function drawnSun(theme: Theme, sky: RealSky): { elevation: number; azimuth: number; afterglow: boolean } {
  const real = sky.sun;
  const evening = real.hourAngle >= 0;
  const side = evening ? sky.sunsetAzimuth : sky.sunriseAzimuth;
  const near = (e: number) => Math.abs(real.elevation - e) < 18;
  const facingAz = sky.facing === "south" ? 180 : 0;
  switch (theme) {
    case "clear":
      if (real.elevation >= 12) return { ...real, afterglow: false };
      // A fine day while the real sun is low or down: mid-morning or mid-afternoon.
      return { elevation: 34, azimuth: facingAz + (evening ? 1 : -1) * (sky.facing === "south" ? 38 : -38), afterglow: false };
    case "golden":
      if (real.elevation >= 1.5 && real.elevation <= 12) return { ...real, afterglow: false };
      if (real.elevation < -10) return { elevation: -14, azimuth: side, afterglow: true };
      return { elevation: 6, azimuth: near(6) ? real.azimuth : side, afterglow: false };
    case "sunset":
      if (real.elevation >= -3 && real.elevation <= 2.5) return { ...real, afterglow: false };
      if (real.elevation < -10) return { elevation: -14, azimuth: side, afterglow: true };
      return { elevation: 0.4, azimuth: near(0) ? real.azimuth : side, afterglow: false };
    case "night":
      return { elevation: Math.min(real.elevation, -22), azimuth: real.azimuth, afterglow: false };
    default:
      return { elevation: real.elevation, azimuth: real.azimuth, afterglow: false };
  }
}

/** The moon as drawn: the real one when it is up at night, a pleasant one in a night theme by day. */
export function drawnMoon(theme: Theme, sky: RealSky, sunElevation: number): SceneSpec["moon"] {
  if (theme === "space" || theme === "storm") return null;
  const dark = sunElevation < -6;
  if (!dark) return null;
  const real = sky.moon;
  if (sky.sun.elevation < -6) {
    return real.elevation > -1 ? { elevation: real.elevation, azimuth: real.azimuth, phase: real.phase } : null;
  }
  // A night sky in daytime (Ciel asleep at noon): a crescent at least, high on the side.
  const phase = real.phase < 0.1 || real.phase > 0.9 ? 0.18 : real.phase;
  const facingAz = sky.facing === "south" ? 180 : 0;
  return { elevation: 36, azimuth: wrap180(facingAz - 42), phase };
}

/** Ciel's place: on the lock screen he stays clear of the clock, lower down. */
export function cielPlacement(prefs: WallpaperPrefs, lockScreen: boolean): CielPlacement {
  if (!lockScreen) return { size: prefs.cielSize, x: prefs.cielX, y: prefs.cielY };
  return { size: Math.min(prefs.cielSize, 0.2), x: prefs.cielX, y: Math.max(prefs.cielY, 0.7) };
}

/** A new sky every day; the clouds drift through the day from there. */
export function seedFor(now: Date): number {
  return hash(now.getFullYear(), now.getMonth(), now.getDate());
}

/** Seconds since local midnight: where the drifting clouds are, for a still or a live frame. */
export function secondsOfDay(now: Date): number {
  return now.getHours() * 3600 + now.getMinutes() * 60 + now.getSeconds() + now.getMilliseconds() / 1000;
}

export interface SpecOptions {
  lockScreen?: boolean;
  place?: Place;
  /** A theme and a phase decided elsewhere (the island's choice, a preview). */
  choice?: SceneChoice;
  /** The sky to draw from (previews); the real one otherwise. */
  sky?: RealSky;
  /** Leave Ciel out. */
  noCiel?: boolean;
}

/** Everything the renderer needs for `mood` at `now`. */
export function sceneSpec(now: Date, mood: BotStateName, prefs: WallpaperPrefs, opts: SpecOptions = {}): SceneSpec {
  const place = opts.place ?? placeFor(prefs.lat, prefs.lon, now);
  const choice = opts.choice ?? chooseScene(now, mood, prefs, place);
  const sky = opts.sky ?? realSky(now, place);
  const sun = drawnSun(choice.theme, sky);
  const lockScreen = opts.lockScreen ?? false;
  return {
    theme: choice.theme,
    phase: choice.phase,
    mood,
    sun: { elevation: sun.elevation, azimuth: sun.azimuth },
    moon: drawnMoon(choice.theme, sky, sun.elevation),
    facing: sky.facing,
    afterglow: sun.afterglow,
    seed: seedFor(now),
    ciel: opts.noCiel ? null : cielPlacement(prefs, lockScreen),
    lockScreen,
  };
}
