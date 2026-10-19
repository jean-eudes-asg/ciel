// The wallpaper's vocabulary and preferences (settings.json key `wallpaper`).
// Rust (settings.rs) owns persistence with the same defaults; this mirrors
// them so the island, the live page and Settings agree on a missing field.
// Everything is off by default: nothing changes on anyone's PC until they
// turn it on.

import type { BotStateName } from "../core/layout";

export type Theme = "clear" | "cloudy" | "golden" | "sunset" | "night" | "space" | "storm";
export type Phase = "night" | "dawn" | "day" | "golden" | "sunset" | "dusk";
export type Source = "drawn" | "photos";

export const THEMES: readonly Theme[] = ["clear", "cloudy", "golden", "sunset", "night", "space", "storm"];
export const PHASES: readonly Phase[] = ["night", "dawn", "day", "golden", "sunset", "dusk"];
export const MOODS: readonly BotStateName[] = [
  "idle", "working", "thinking", "searching", "approval", "question",
  "error", "finished", "ratelimit", "sleeping", "dizzy",
];

export interface WallpaperPrefs {
  /** Apply to the desktop. */
  desktop: boolean;
  /** Apply to the lock screen (always a still picture). */
  lockScreen: boolean;
  /** The desktop is animated; false = still pictures. */
  live: boolean;
  /** Live pauses on battery. */
  pauseOnBattery: boolean;
  /** Null = guessed from the time zone. */
  lat: number | null;
  lon: number | null;
  /** Ciel's width as a fraction of the screen height. */
  cielSize: number;
  /** Ciel's centre, fractions of the width and the height. */
  cielX: number;
  cielY: number;
  /** Still pictures are redrawn at least this often. */
  refreshMinutes: number;
  /** Mood → theme, or "auto"; a missing mood is "auto". */
  moods: Partial<Record<BotStateName, Theme | "auto">>;
  /** Theme → source; a missing theme is drawn. */
  sources: Partial<Record<Theme, Source>>;
}

export const DEFAULT_WALLPAPER: WallpaperPrefs = {
  desktop: false,
  lockScreen: false,
  live: true,
  pauseOnBattery: true,
  lat: null,
  lon: null,
  cielSize: 0.2,
  cielX: 0.5,
  cielY: 0.66,
  refreshMinutes: 15,
  moods: {},
  sources: {},
};

export const isTheme = (v: unknown): v is Theme => typeof v === "string" && (THEMES as readonly string[]).includes(v);

const num = (v: unknown, fallback: number, min: number, max: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;

const coord = (v: unknown, limit: number): number | null =>
  typeof v === "number" && Number.isFinite(v) && Math.abs(v) <= limit ? v : null;

/**
 * The preferences as saved, with every missing or broken field put back to
 * its default. Unknown keys are kept (Rust keeps them too).
 */
export function wallpaperPrefs(raw: unknown): WallpaperPrefs {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_WALLPAPER;
  const moods: WallpaperPrefs["moods"] = {};
  if (r.moods && typeof r.moods === "object") {
    for (const [mood, theme] of Object.entries(r.moods as Record<string, unknown>)) {
      if ((MOODS as readonly string[]).includes(mood) && (theme === "auto" || isTheme(theme))) {
        moods[mood as BotStateName] = theme;
      }
    }
  }
  const sources: WallpaperPrefs["sources"] = {};
  if (r.sources && typeof r.sources === "object") {
    for (const [theme, source] of Object.entries(r.sources as Record<string, unknown>)) {
      if (isTheme(theme) && (source === "drawn" || source === "photos")) sources[theme] = source;
    }
  }
  return {
    ...r,
    desktop: r.desktop === true,
    lockScreen: r.lockScreen === true,
    live: typeof r.live === "boolean" ? r.live : d.live,
    pauseOnBattery: typeof r.pauseOnBattery === "boolean" ? r.pauseOnBattery : d.pauseOnBattery,
    lat: coord(r.lat, 90),
    lon: coord(r.lon, 180),
    cielSize: num(r.cielSize, d.cielSize, 0.05, 0.6),
    cielX: num(r.cielX, d.cielX, 0, 1),
    cielY: num(r.cielY, d.cielY, 0, 1),
    refreshMinutes: num(r.refreshMinutes, d.refreshMinutes, 1, 24 * 60),
    moods,
    sources,
  };
}
