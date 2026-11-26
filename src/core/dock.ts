// The dock (src/dock, src-tauri/src/dock): what Rust sends it, what the island
// tells it, and the little logic drawn from them. Types and pure functions
// only: no DOM, no Tauri, so Node runs the tests.

import type { CielNotification } from "./notifications";
import type { CodexPlanUsage, PlanUsage } from "./plan";
import type { SessionRow } from "./sessions";
import { lineAt, type LyricLine } from "./lyrics";
import type { SpotifyState } from "./spotify";
import type { IntegrationInfo } from "./state";

export interface DockPrefs {
  on: boolean;
  /** While the dock shows, the Windows taskbar of the main screen is hidden; on GNOME, GNOME's own dock is off. */
  hideTaskbar: boolean;
  /** The icons grow under the pointer. */
  magnify: boolean;
  /** Windows' own buttons beside Start (which searches too). */
  taskView: boolean;
  widgets: boolean;
  /** What plays is in the dock, with its words, and no longer in the island. */
  music: boolean;
  /** GNOME: each screen's dock stays out of sight until the pointer reaches the bottom of that screen. */
  autoHide: boolean;
  /** Windows: the dock's own pins (paths, in order); null while they are the taskbar's. GNOME's are its favourites. */
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
  /** What plays, while the dock has the music (DockPrefs.music). */
  playing: DockPlaying | null;
  /** Each session that isn't idle, a dot under Ciel: who needs the user first. */
  crew: CrewMate[];
  /** The Claude plan's usage, from Claude Code's status line: the gauge beside Ciel. */
  plan: PlanUsage | null;
}

/** The song in the dock: its player, where it is, and its words. */
export interface DockPlaying {
  title: string;
  artist: string;
  playing: boolean;
  /** Whose buttons the dock's are. */
  source: DockSource;
  /** Its cover and title bring the player forward: Spotify, or the YouTube Music tab (not another app playing). */
  opens: boolean;
  /** Seconds at `positionAt` (Unix ms), running on from there while playing. */
  position: number;
  positionAt: number;
  /** Seconds, 0 when the player doesn't say. */
  duration: number;
  /** Its timed words, when lyrics are on and lrclib.net has them. */
  lyrics: LyricLine[];
}

export type DockSource = "spotify" | "ytmusic";

/** What the island hands dockSnapshot: the player that plays (or last did) and the song's words. */
export interface DockMusic {
  source: DockSource;
  state: SpotifyState;
  lyrics: LyricLine[];
}

/** One agent's session, as the dock shows it: a dot under Ciel, a line of his label. */
export interface CrewMate {
  key: string;
  agent: string;
  project: string;
  /** What it is doing, or the last thing it said. */
  line: string;
  phase: "asking" | "working" | "done" | "error";
  /** When the phase last changed, ms since 1970. */
  since: number;
}

/** How many sessions have a dot under Ciel. */
export const CREW_MAX = 5;
/** How long one that finished or failed stays in the dock. */
export const CREW_STAYS_MS = 60_000;

/** The sessions now: the ones done or failed for a while have gone. */
export function crewNow(crew: readonly CrewMate[], now: number): CrewMate[] {
  return crew.filter((m) => (m.phase !== "done" && m.phase !== "error") || now - m.since < CREW_STAYS_MS);
}

export const DOCK_EVENTS = {
  /** Rust → dock: the row, and whether it is on screen (dock/mod.rs STATE_EVENT). */
  state: "dock-state",
  /** Rust → dock: the sound, the network and the battery alone (SYSTEM_EVENT). */
  system: "dock-system",
  /** Rust → one dock (GNOME): its bar goes out of sight (true, auto-hide), or comes up. */
  away: "dock-away",
  /** Island → dock: its agents and what plays. */
  island: "dock-island",
  /** Island → dock: the cover of what plays (a data: URL), or null. */
  art: "dock-art",
  /** Dock → island: the page is there, tell it everything. */
  ready: "dock-ready",
  /** Dock → island: open on the agents, or on what waits for the user. */
  open: "dock-open",
  /** Island → the panel over the dock (GNOME): the notifications and the calendar's events. */
  panel: "dock-panel-island",
  /** Panel → island: the page is there, tell it everything. */
  panelReady: "dock-panel-ready",
  /** Panel → island: a notification opened or dismissed ({ id, action }), all cleared (id null). */
  note: "dock-note",
  /** Panel → island: the user's click on Allow or Deny ({ requestId, decision }), for the island to make. */
  decide: "dock-decide",
} as const;

/** The request that waits, as the hover window over Ciel shows it. */
export interface DockRequest {
  requestId: string;
  agent: string;
  project: string;
  tool: string;
  command: string;
  /** A question (AskUserQuestion): answered in the island, not here. */
  question: boolean;
}

/** What the panel over the dock shows of the island's. */
export interface DockPanelSnapshot {
  notifications: CielNotification[];
  /** The Calendar pill as the island has it (its month), or null. */
  calendar: IntegrationInfo | null;
  /** The permission or question that waits for the user, if one does. */
  request: DockRequest | null;
  /** The Codex plan, when Codex has told it. */
  codex: CodexPlanUsage | null;
}

export const EMPTY_SNAPSHOT: DockSnapshot = {
  agents: { asking: 0, working: 0, done: 0, error: 0 },
  asker: null,
  playing: null,
  crew: [],
  plan: null,
};

/** The island's sessions and player, as the dock shows them. */
export function dockSnapshot(rows: readonly SessionRow[], music: DockMusic | null, plan: PlanUsage | null = null): DockSnapshot {
  const agents = { asking: 0, working: 0, done: 0, error: 0 };
  const crew: CrewMate[] = [];
  for (const row of rows) {
    if (row.phase === "idle") continue;
    agents[row.phase] += 1;
    crew.push({ key: row.key, agent: row.agent, project: row.project, line: row.line, phase: row.phase, since: row.since });
  }
  const asker = rows.find((r) => r.phase === "asking")?.agent ?? null;
  const s = music?.state;
  const track = s?.track;
  const playing = music && s && track && track.title && !track.id.startsWith("spotify:ad:")
    ? {
      title: track.title, artist: track.artist, playing: s.playing, source: music.source,
      opens: music.source === "ytmusic" || s.spotify,
      position: s.position, positionAt: s.positionAt, duration: track.duration, lyrics: music.lyrics,
    }
    : null;
  return { agents, asker, playing, crew, plan };
}

/** Where the song is now, in seconds. */
export function playedAt(p: DockPlaying, nowMs: number): number {
  const at = p.playing ? p.position + Math.max(0, nowMs - p.positionAt) / 1000 : p.position;
  return p.duration > 0 ? Math.min(at, p.duration) : at;
}

/** The line sung now ("" between lines, or without words), and in how many ms the next one comes (null: none). */
export function dockLyric(p: DockPlaying, nowMs: number): { text: string; nextIn: number | null } {
  const at = playedAt(p, nowMs);
  const i = lineAt(p.lyrics, at);
  const next = p.lyrics[i + 1];
  // lineAt takes a line from 0.15 s before its time.
  return { text: i >= 0 ? p.lyrics[i].text : "", nextIn: next && p.playing ? Math.max(0, (next.t - 0.15 - at) * 1000) : null };
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

/** Words before a command's name: wrappers, and what shells put first. */
const LEAD_WORDS = new Set(["do", "then", "else", "elif", "time", "sudo", "env", "nohup", "exec", "command", "builtin", "timeout", "nice", "!"]);
/** Pieces of a command line that run nothing worth naming. */
const NOT_A_NAME = new Set(["for", "while", "until", "if", "case", "cd", "export", "set", "source", ".", "done", "fi", "esac", "{", "}"]);

/**
 * The name of what a shell line runs, as the dock says it: "npm" for
 * `cd app && npm test 2>&1`, "python3" for `S=/tmp/x; python3 - <<E`. Empty
 * when it can't tell (the line may come cut short).
 */
export function commandName(command: string): string {
  for (const piece of command.split(/&&|\|\||[;|&\n]/)) {
    const words = piece.trim().split(/\s+/).filter(Boolean);
    let i = 0;
    // VAR=value, sudo, timeout 60 and their flags.
    while (i < words.length && (/^[A-Za-z_]\w*=/.test(words[i]) || LEAD_WORDS.has(words[i]) || (i > 0 && LEAD_WORDS.has(words[i - 1]) && /^(-|\d)/.test(words[i])))) i++;
    const word = (words[i] ?? "").replace(/^["'(]+|["')]+$/g, "");
    if (!word || NOT_A_NAME.has(word)) continue;
    const name = word.slice(word.lastIndexOf("/") + 1);
    if (name) return name;
  }
  return "";
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
