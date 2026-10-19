// What the island, the live wallpaper windows and Settings say to each other.

import { Bridge } from "../core/bridge";
import type { BotStateName } from "../core/layout";
import type { Phase, Theme } from "./prefs";

export const WALLPAPER_EVENTS = {
  /** Island → live windows (and Settings' preview): what to draw. */
  scene: "wallpaper-scene",
  /** Rust → live windows: stop drawing (fullscreen app, battery, locked, display off). */
  pause: "wallpaper-live-pause",
  /** Rust → island: the session was locked or unlocked. */
  session: "wallpaper-session",
  /** Live window → island: it (re)loaded and knows nothing yet. */
  ready: "wallpaper-ready",
  /** Settings → island: "Apply now". */
  apply: "wallpaper-apply",
} as const;

/** What a live window needs to draw the same sky as the stills. */
export interface LiveScene {
  mood: BotStateName;
  theme: Theme;
  phase: Phase;
  /** A cached photo to draw instead of the sky. */
  photoPath: string | null;
  /** The user's coordinates (null: guessed from the time zone, there too). */
  lat: number | null;
  lon: number | null;
  ciel: { size: number; x: number; y: number };
}

/** A cached photo as a bitmap (its bytes come through Rust; file URLs are refused). */
export async function decodePhoto(path: string): Promise<ImageBitmap | null> {
  const bytes = await Bridge.wallpaperPhotoBytes(path);
  if (!bytes) return null;
  try {
    return await createImageBitmap(new Blob([bytes as BlobPart]));
  } catch {
    return null;
  }
}
