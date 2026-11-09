// What the Tools tab's backends send (src-tauri/src/clipboard.rs, timers.rs,
// controls.rs, weather.rs, lyrics.rs, shelf.rs). Types only: no DOM, no
// Tauri, so the pure parts can be tested in Node.

export type ToolId = "clipboard" | "shelf" | "timers" | "controls" | "weather" | "calendar";

export interface ToolPrefs {
  /** Keep what is copied (clipboard.rs). */
  clipboard: boolean;
  /** The words of the song playing, from lrclib.net (lyrics.rs). */
  lyrics: boolean;
  /** The weather's place, as picked; "" for none. */
  weatherPlace: string;
  weatherLat: number;
  weatherLon: number;
  /** "auto" follows the language's region, until a place is picked: then its country's. */
  weatherUnits: "auto" | "c" | "f";
  /** A card when rain is about to start. */
  rainWarnings: boolean;
  /** The sky wallpaper wears the real weather rather than Ciel's mood. */
  skyWeather: boolean;
  /** Tools → Calendar's other cities. */
  worldClocks: WorldClock[];
}

export interface WorldClock {
  name: string;
  /** IANA: "Africa/Porto-Novo". */
  tz: string;
}

export const DEFAULT_TOOL_PREFS: ToolPrefs = {
  clipboard: false,
  lyrics: false,
  weatherPlace: "",
  weatherLat: 0,
  weatherLon: 0,
  weatherUnits: "auto",
  rainWarnings: false,
  skyWeather: false,
  worldClocks: [],
};

// ── Clipboard ─────────────────────────────────────────────────────────────────

export interface ClipItem {
  id: number;
  kind: "text" | "image" | "files";
  /** The text; the paths, one a line, for files. */
  text: string;
  thumb?: string;
  width: number;
  height: number;
  at: number;
  pinned: boolean;
  app: string;
  /** A one-time code goes then (Unix ms). */
  expires?: number;
}

export interface ClipState {
  on: boolean;
  /** This session can see the copies (a Wayland session without wl-paste can't). */
  watching: boolean;
  items: ClipItem[];
}

// ── Timers ────────────────────────────────────────────────────────────────────

export interface TimerItem {
  id: string;
  kind: "timer" | "reminder" | "focus";
  label: string;
  /** Unix ms: when it rings; 0 while paused. */
  due: number;
  duration: number;
  /** Ms left, while paused. */
  left?: number;
  repeat: "" | "daily" | "weekdays" | "weekly";
  ringing: boolean;
  phase: "" | "focus" | "break";
  round: number;
  focusMs: number;
  breakMs: number;
}

// ── Controls ──────────────────────────────────────────────────────────────────

export interface Controls {
  volume: number | null;
  muted: boolean | null;
  micMuted: boolean | null;
  mics: number;
  brightness: number | null;
  wifi: boolean | null;
  bluetooth: boolean | null;
  nightLight: boolean | null;
  awake: boolean;
  awakeUntil: number | null;
  /** Speakers to choose from; empty when there is no choice to make here. */
  outputs: Device[];
  /** null: no network manager to ask. */
  network: { kind: "wired" | "wifi" | "mobile" | ""; name: string } | null;
  connections: Connection[];
  powerMode: PowerMode | null;
  powerModes: PowerMode[];
  dark: boolean | null;
  /** Notifications held: GNOME's banners and Ciel's own cards. */
  dnd: boolean;
  battery: { percent: number; charging: boolean } | null;
}

export type PowerMode = "power-saver" | "balanced" | "performance";

export interface Device {
  id: string;
  name: string;
  default: boolean;
}

export interface Connection {
  id: string;
  name: string;
  kind: "wired" | "wifi" | "vpn";
  active: boolean;
}

/** One of the user's own apps (GNOME's dock favourites, the taskbar's pins). */
export interface LauncherApp {
  id: string;
  name: string;
  icon: string | null;
}

// ── Weather ───────────────────────────────────────────────────────────────────

export interface Place {
  name: string;
  detail: string;
  lat: number;
  lon: number;
  /** "FR"; "" when Open-Meteo doesn't say. */
  countryCode: string;
  /** "Europe/Paris"; "" when Open-Meteo doesn't say. */
  timezone: string;
}

export interface WeatherNow {
  temp: number;
  feels: number;
  code: number;
  isDay: boolean;
  /** km/h. */
  wind: number;
  humidity: number;
}

export interface WeatherHour {
  t: number;
  temp: number;
  code: number;
  pop: number;
  isDay: boolean;
}

export interface WeatherDay {
  t: number;
  code: number;
  max: number;
  min: number;
  pop: number;
  sunrise: number;
  sunset: number;
}

export interface Weather {
  place: string;
  lat: number;
  lon: number;
  at: number;
  now: WeatherNow;
  hours: WeatherHour[];
  days: WeatherDay[];
  rainAt: number | null;
  raining: boolean;
  rainUntil: number | null;
}

// ── Music ─────────────────────────────────────────────────────────────────────

export interface Lyrics {
  synced: string | null;
  plain: string | null;
  instrumental: boolean;
}

// ── Shelf ─────────────────────────────────────────────────────────────────────

export interface ShelfFile {
  name: string;
  path: string;
  size: number;
  at: number;
  preview: string | null;
}
