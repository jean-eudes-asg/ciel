// The dock (src/dock, src-tauri/src/dock): what Rust sends it, what the island
// tells it, and the little logic drawn from them. Types and pure functions
// only: no DOM, no Tauri, so Node runs the tests.

import type { SessionRow } from "./sessions";
import type { SpotifyState } from "./spotify";

export interface DockPrefs {
  on: boolean;
  /** Windows: the taskbar of the main screen hidden while the dock shows. */
  hideTaskbar: boolean;
  /** The icons grow under the pointer. */
  magnify: boolean;
  /** Windows' own buttons beside Start (which searches too). */
  taskView: boolean;
  widgets: boolean;
  /** The dock's own pins (paths, in order); null while they are the taskbar's. */
  pins: string[] | null;
}

/** Windows' panels the dock opens, as their keys do (dock/win.rs panel). */
export type DockPanel = "taskView" | "widgets" | "quickSettings" | "notifications" | "desktop";

/** The bar's corner (dock/mod.rs SystemIcons). */
export interface SystemIcons {
  /** 0…1; null without speakers. */
  volume: number | null;
  muted: boolean;
  /** "wifi", "wired", "mobile", or "" when not connected. */
  network: string;
  networkName: string;
  /** 0…100; null without a battery. */
  battery: number | null;
  charging: boolean;
}

export const NO_SYSTEM: SystemIcons = { volume: null, muted: false, network: "", networkName: "", battery: null, charging: false };

/** An app of the row (dock/mod.rs DockApp). */
export interface DockApp {
  /** The pinned shortcut's path, or the program's for an app only open. */
  id: string;
  name: string;
  /** A data: URL, or null when Windows has none. */
  icon: string | null;
  pinned: boolean;
  /** How many of its windows are open. */
  windows: number;
  /** One of them is the window in front. */
  active: boolean;
  /** One of them flashes, asking to be looked at. */
  attention: boolean;
}

export interface DockState {
  apps: DockApp[];
  /** The Windows taskbar is hidden: the dock offers to show it for a moment. */
  taskbarHidden: boolean;
  /** On screen; false while off or out of a full-screen app's way (the clock rests). */
  shown: boolean;
  system: SystemIcons;
}

/** What the island tells the dock (src/island/dock.ts). */
export interface DockSnapshot {
  /** The sessions that need the user, work, or are done. */
  agents: { asking: number; working: number; done: number; error: number };
  /** The agent of the session waiting for an answer, if one does. */
  asker: string | null;
  /** What plays, from the music pill, while it is on. */
  playing: { title: string; artist: string; playing: boolean } | null;
}

export const DOCK_EVENTS = {
  /** Rust → dock: the row, and whether it is on screen (dock/mod.rs STATE_EVENT). */
  state: "dock-state",
  /** Rust → dock: the sound, the network and the battery alone (SYSTEM_EVENT). */
  system: "dock-system",
  /** Island → dock: its agents and what plays. */
  island: "dock-island",
  /** Dock → island: the page is there, tell it everything. */
  ready: "dock-ready",
  /** Dock → island: open on the agents, or on what waits for the user. */
  open: "dock-open",
} as const;

export const EMPTY_SNAPSHOT: DockSnapshot = {
  agents: { asking: 0, working: 0, done: 0, error: 0 },
  asker: null,
  playing: null,
};

/** The island's sessions and player, as the dock shows them. */
export function dockSnapshot(rows: readonly SessionRow[], music: SpotifyState | null): DockSnapshot {
  const agents = { asking: 0, working: 0, done: 0, error: 0 };
  for (const row of rows) {
    if (row.phase !== "idle") agents[row.phase] += 1;
  }
  const asker = rows.find((r) => r.phase === "asking")?.agent ?? null;
  const track = music?.track;
  const playing = track && track.title && !track.id.startsWith("spotify:ad:")
    ? { title: track.title, artist: track.artist, playing: music.playing }
    : null;
  return { agents, asker, playing };
}

/** What the agents' tile says: who waits for the user first, then how many work. */
export type AgentsLine =
  | { kind: "asking"; agent: string }
  | { kind: "working"; count: number }
  | { kind: "quiet" };

export function agentsLine(s: DockSnapshot): AgentsLine {
  if (s.agents.asking > 0) return { kind: "asking", agent: s.asker ?? "" };
  if (s.agents.working > 0) return { kind: "working", count: s.agents.working };
  return { kind: "quiet" };
}

/** The clock's hour as Settings → Clock wants it ("auto": the language's). */
export function hour12(format: "auto" | "12h" | "24h" | undefined): boolean | undefined {
  return format === "12h" ? true : format === "24h" ? false : undefined;
}

/** Milliseconds to the next minute, just past it, for a clock that ticks once a minute. */
export function untilNextMinute(now: number): number {
  return 60_000 - (now % 60_000) + 50;
}

// ── Icons that grow under the pointer ────────────────────────────────────────

/** How large an icon grows at most, and how far from the pointer (CSS pixels) it still does. */
export const MAGNIFY = { max: 1.7, reach: 100 } as const;

/** An icon's size, `distance` from the pointer: full at it, back to 1 at `reach`, smoothly. */
export function magnifyScale(distance: number, max: number = MAGNIFY.max, reach: number = MAGNIFY.reach): number {
  const d = Math.abs(distance) / reach;
  if (!(d < 1)) return 1;
  return 1 + (max - 1) * (Math.cos(d * Math.PI) + 1) / 2;
}

/** An item of the bar at rest: where it starts and how wide it is (CSS pixels), and the size it takes. */
export interface MagnifyItem {
  left: number;
  width: number;
  scale: number;
}

/**
 * The bar laid out again with its items grown, as if each took its width
 * times its scale, the point under the pointer staying where it is: how far
 * each item moves (from its centre at rest), and how much wider the bar gets
 * on each side. Moving items rather than resizing them keeps the bar's own
 * size, so the window doesn't follow the pointer around.
 */
export function magnifyLayout(items: readonly MagnifyItem[], pointer: number): { shifts: number[]; growLeft: number; growRight: number } {
  if (items.length === 0) return { shifts: [], growLeft: 0, growRight: 0 };
  // Where each item starts once the ones before it have grown (gaps kept).
  const starts: number[] = [items[0].left];
  for (let i = 1; i < items.length; i++) {
    const prev = items[i - 1];
    const gap = items[i].left - (prev.left + prev.width);
    starts.push(starts[i - 1] + prev.width * prev.scale + gap);
  }
  // The pointer's place in the grown row: in an item, the same fraction of it;
  // in a gap or past an end, the same distance from the item before.
  let moved = pointer;
  if (pointer >= items[0].left) {
    let k = 0;
    while (k + 1 < items.length && items[k + 1].left <= pointer) k++;
    const it = items[k];
    const into = pointer - it.left;
    moved = into < it.width
      ? starts[k] + (into / it.width) * it.width * it.scale
      : starts[k] + it.width * it.scale + (into - it.width);
  }
  const offset = pointer - moved;
  const shifts = items.map((it, i) => starts[i] + offset + (it.width * it.scale) / 2 - (it.left + it.width / 2));
  const last = items.length - 1;
  const end = starts[last] + items[last].width * items[last].scale + offset;
  return {
    shifts,
    growLeft: Math.max(0, items[0].left - (starts[0] + offset)),
    growRight: Math.max(0, end - (items[last].left + items[last].width)),
  };
}

/** Where a dragged pin goes: before the first pin whose middle is right of the pointer. */
export function dropIndex(centres: readonly number[], pointer: number): number {
  const at = centres.findIndex((c) => pointer < c);
  return at === -1 ? centres.length : at;
}

/** The sound icon for a volume: off, then three steps. */
export function volumeLevel(volume: number | null, muted: boolean): 0 | 1 | 2 | 3 {
  if (volume === null || muted || volume <= 0) return 0;
  return volume < 0.34 ? 1 : volume < 0.67 ? 2 : 3;
}
