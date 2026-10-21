// Thin wrapper over the Tauri commands/events. Every call is a no-op when the
// page is opened in a plain browser, so the island can be iterated on with
// `npm run dev` alone.

import { invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Settings } from "./state";
import type { RecapHistory, RecapPrefs } from "../recap/summary";
import type { SpotifyState } from "./spotify";
import type { FinanceFolderInfo, FinancePoll, NewExpense } from "./finance";

export const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ── Demo mode (src/demo) ──────────────────────────────────────────────────────

/**
 * While the demo runs, every command and every event from Rust passes its gate
 * first: the demo answers from made-up data and swallows anything that would
 * write, so nothing it shows comes from the user's files, keychain or agents,
 * and nothing reaches them. Null the rest of the time.
 */
export interface BridgeGate {
  /** `{ value }` answers the command here (a rejected promise throws); undefined lets it through. */
  command(cmd: string, args?: Record<string, unknown>): { value: unknown } | undefined;
  /** False drops an event Rust sent. */
  event(name: string, payload: unknown): boolean;
}

let gate: BridgeGate | null = null;

export function setBridgeGate(next: BridgeGate | null) {
  gate = next;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  const routed = gate?.command(cmd, args);
  if (routed) {
    try {
      return (await routed.value) as T;
    } catch (err) {
      console.error(`[ciel] ${cmd} failed`, err);
      return null;
    }
  }
  if (!IS_TAURI) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    console.error(`[ciel] ${cmd} failed`, err);
    return null;
  }
}

export interface BootInfo {
  settings: Settings;
  /** Logical screen rect of the monitor the island lives on. */
  screen: { x: number; y: number; width: number; height: number; scale: number };
  version: string;
  hookPath: string;
  /** False where the OS has no global cursor (Wayland): see Island.followPageCursor. */
  cursorPoll: boolean;
  /** Ciel sees a press anywhere on screen (Windows, X11), not on Wayland. */
  clicksOutside: boolean;
}

export const Bridge = {
  boot: () => call<BootInfo>("boot"),
  /** The system's languages as the webview sees them, for Rust's own texts (i18n.rs). */
  setSystemLanguages: (languages: string[]) => call<void>("set_system_languages", { languages }),

  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),

  /** Shrink the window down to the invisible wake strip (hidden) or back to full. */
  setCollapsed: (collapsed: boolean) => call<void>("set_collapsed", { collapsed }),

  /**
   * Pushes the island shape in window coordinates. Rust flips click-through from
   * its own cursor poll, so the flag is never a frame behind a click.
   */
  /** The window's height while the island is open: the panel's, or more for a long notifications list. */
  setPanelHeight: (height: number) => call<void>("set_panel_height", { height }),
  setIslandRect: (x: number, y: number, width: number, height: number) =>
    call<void>("set_island_rect", { x, y, width, height }),

  /** Give the window keyboard focus (chat field) and take it away again. */
  focusWindow: (focused: boolean) => call<void>("focus_window", { focused }),

  reposition: () => call<void>("reposition"),
  fitZoom: (dpr: number) => call<boolean>("fit_zoom", { dpr }),

  /** Displays the island can be pinned to: `key` is what `settings.screen` stores. */
  listMonitors: () => call<{ key: string; label: string }[]>("list_monitors"),

  openUrl: (url: string) => call<void>("open_url", { url }),

  /** Which of these sounds have a file of the user's own in the sounds folder. */
  customSounds: (names: readonly string[]) => call<string[]>("custom_sounds", { names }),
  /** The user's file for one sound, as raw bytes. */
  customSound: (name: string) => call<ArrayBuffer>("custom_sound", { name }),
  revealSoundsFolder: () => call<void>("reveal_sounds_folder"),
  /** Tells the island to read the sounds folder again ("sounds-changed"). */
  reloadSounds: () => call<void>("reload_sounds"),

  /** "Open terminal" → opens the folder in VS Code when `code` is on PATH. */
  openInVSCode: (path: string | null) => call<boolean>("open_in_vscode", { path }),

  /**
   * "Open terminal": the window the session runs in when Rust found it
   * (Windows; Linux on X11 and KDE Plasma), else the folder in VS Code.
   */
  openSession: (sessionId: string | null, path: string | null) =>
    call<boolean>("open_session", { sessionId, path }),

  /** The Claude desktop app, for the Claude Desktop pill (Windows only). */
  openClaudeDesktop: () => call<boolean>("open_claude_desktop"),

  /** The diff card's ↗: an existing file, in VS Code; never launched by its type. */
  openFileInVSCode: (path: string) => call<boolean>("open_file_in_vscode", { path }),

  quit: () => call<void>("quit_app"),

  openSettingsWindow: () => call<void>("open_settings_window"),

  /** Writes to %LOCALAPPDATA%\Ciel\ciel.log, next to the Rust lines. */
  log: (message: string) => call<void>("log_line", { message }),

  // ── Claude Code hooks ─────────────────────────────────────────────────────
  hooksStatus: () => call<HookStatus>("hooks_status"),
  /** Pill ID → whether that agent's hooks reach Ciel (read-only, Mac #183). */
  agentHooksStatus: () => call<Record<string, boolean>>("agent_hooks_status"),
  /** Diff to show before anything is written. `install: false` previews removal. */
  hooksPreview: (install: boolean) => callOrThrow<HookPreview>("hooks_preview", { install }),
  /**
   * Writes ~/.claude/settings.json — only ever after an explicit click, and only
   * when the file still matches the preview the user looked at.
   */
  hooksApply: (install: boolean, fingerprint: string) =>
    callOrThrow<string>("hooks_apply", { install, fingerprint }),

  // ── Other agents (Gemini CLI, Codex, Cursor…) ─────────────────────────────
  agentHooksList: () => call<AgentHookStatus[]>("agent_hooks_list"),
  /** Diff to show before anything is written. `install: false` previews removal. */
  agentHooksPreview: (agent: string, install: boolean) =>
    callOrThrow<AgentHookPlan>("agent_hooks_preview", { agent, install }),
  /**
   * Writes the agent's config — only ever after an explicit click, and only when
   * it still matches the preview the user looked at. Returns the backups taken.
   */
  agentHooksApply: (agent: string, install: boolean, fingerprint: string) =>
    callOrThrow<string>("agent_hooks_apply", { agent, install, fingerprint }),
  // ── Plan usage: the status line relay, installed apart from the hooks ──────
  /** Diff of the status line change. `install: false` previews taking the relay out. */
  statusLinePreview: (install: boolean) => callOrThrow<HookPreview>("status_line_preview", { install }),
  /** Same rules as hooksApply: an explicit click, and only for the diff that was shown. */
  statusLineApply: (install: boolean, fingerprint: string) =>
    callOrThrow<string>("status_line_apply", { install, fingerprint }),
  /**
   * Asks the Codex CLI (`codex app-server`) for its plan limits, as Codex's
   * /status does. The raw `account/rateLimits/read` result, or null when Codex
   * is missing, not signed in or slow (15 s).
   */
  codexPlanUsage: () => call<unknown>("codex_plan_usage"),

  approvalDecision: (requestId: string, decision: "allow" | "deny") =>
    call<void>("approval_decision", { requestId, decision }),
  /** "The card is up" — until this lands the relay only waits a moment. */
  approvalAck: (requestId: string) => call<void>("approval_ack", { requestId }),
  /** Answers a question Claude Code asked: question text → chosen label. */
  approvalAnswer: (requestId: string, answers: Record<string, string | string[]>) =>
    call<void>("approval_answer", { requestId, answers }),

  /** "Nobody can act on this" — Claude Code asks in the terminal right away. */
  approvalDecline: (requestId: string) => call<void>("approval_decline", { requestId }),

  // ── Chat, files, secrets ──────────────────────────────────────────────────
  /** One chat turn. The API key and any file bytes never leave Rust. */
  chatSend: (query: string, context: ChatContext | null) =>
    callOrThrow<{ text: string }>("chat_send", { query, context }),
  chatReset: () => call<void>("chat_reset"),
  /**
   * The models a provider offers, for the picker in the chat view. Rust asks
   * the provider only when it has a key (or a server address).
   */
  chatModels: (provider: string) => callOrThrow<ModelInfo[]>("chat_models", { provider }),
  /** Settings → Local models → Connect: does the server answer, and with which models? */
  /** The custom server's key, bound to the address it is entered for. */
  localSetKey: (url: string, key: string) => call<void>("local_set_key", { url, key }),

  localConnect: (provider: "ollama" | "lmstudio" | "custom", url: string) =>
    callOrThrow<LocalServer>("local_connect", { provider, url }),
  /** Copies a dropped file into the inbox. */
  ingestFile: (path: string) => callOrThrow<DroppedFile>("ingest_file", { path }),
  /** Only ever tells you whether a key exists — never its value. */
  secretPresent: (key: string) => call<boolean>("secret_present", { key }),
  secretSet: (key: string, value: string) => callOrThrow<void>("secret_set", { key, value }),
  secretClear: (key: string) => callOrThrow<void>("secret_clear", { key }),

  // ── Integrations ──────────────────────────────────────────────────────────
  refreshIntegration: (id: string) => call<void>("refresh_integration", { id }),
  /** The GitHub card is on screen: refetch that part if it is stale. */
  githubRefresh: (section: "pulse" | "activity") => call<void>("github_refresh", { section }),
  /** Opens the configured n8n instance in the browser. */
  openN8n: () => call<void>("open_n8n"),

  /** Tray → Pause. Stops the integration pollers, not just the island. */
  setPaused: (paused: boolean) => call<void>("set_paused", { paused }),

  // ── Global shortcuts ──────────────────────────────────────────────────────
  /** How each global shortcut went when Rust last registered them. */
  shortcutsStatus: () => call<ShortcutsReport>("shortcuts_status"),
  /** Lets go of every global shortcut while Settings records a new one. */
  shortcutsSuspend: (suspended: boolean) => call<void>("shortcuts_suspend", { suspended }),

  // ── Weekly recap ──────────────────────────────────────────────────────────
  /** Turns and decisions from `since` (Unix seconds) on, plus the recap prefs. */
  recapHistory: (since: number) => call<RecapHistory>("recap_history", { since: Math.floor(since) }),
  recapPrefs: () => call<RecapPrefs>("recap_prefs"),
  recapSetEnabled: (enabled: boolean) => call<void>("recap_set_enabled", { enabled }),
  recapSetHideProjects: (hide: boolean) => call<void>("recap_set_hide_projects", { hide }),
  /** `week` is the Monday (YYYY-MM-DD) the recap opened on its own for. */
  recapMarkShown: (week: string) => call<void>("recap_mark_shown", { week }),
  recapClear: () => call<void>("recap_clear"),
  /** Writes the PNG (a data URL) into Pictures or Downloads; returns its path. */
  recapSavePng: (data: string, week: string) => callOrThrow<string>("recap_save_png", { data, week }),
  /** Opens the folder of the image saved last. */
  recapRevealSaved: () => call<void>("recap_reveal_saved"),

  // ── Ciel on the desktop (src-tauri/src/desktop.rs) ───────────────────────
  desktopInfo: () => call<DesktopInfo>("desktop_cloud_info"),
  /** Dragged out of the island: (x, y) is the pointer in island-window coordinates. */
  desktopPickUp: (x: number, y: number) => call<boolean>("desktop_cloud_pick_up", { x, y }),
  /** Linux: the pointer moved during that drag (Windows carries him from Rust). */
  desktopCarry: (x: number, y: number) => call<void>("desktop_cloud_carry", { x, y }),
  desktopCarryEnd: (x: number, y: number) => call<void>("desktop_cloud_carry_end", { x, y }),
  /** A drag started on the desktop Ciel; resolves to his top-left corner. */
  desktopDragBegin: () => call<[number, number] | null>("desktop_cloud_drag_begin"),
  /** X11: top-left corner, physical pixels. */
  desktopDragMove: (x: number, y: number) => call<void>("desktop_cloud_drag_move", { x, y }),
  desktopDragEnd: (x: number, y: number) => call<void>("desktop_cloud_drag_end", { x, y }),
  /** From the island to his spot. False: no spot on any connected display.
   *  `anywhere`: the first-visit corner then, rather than staying home. */
  desktopFlyOut: (anywhere = false) => call<boolean>("desktop_cloud_fly_out", { anywhere }),
  /** To the island, then hidden. `forget`: he lives in the island again. */
  desktopFlyHome: (forget: boolean) => call<boolean>("desktop_cloud_fly_home", { forget }),
  /** Asleep, the cursor poll stops. */
  desktopSetAsleep: (asleep: boolean) => call<void>("desktop_cloud_set_asleep", { asleep }),

  // ── Spotify (src-tauri/src/spotify.rs) ────────────────────────────────────
  /** Reads the player again (the card came on screen) and reports it. */
  spotifyRefresh: () => call<SpotifyState | null>("spotify_refresh"),
  /** True when Spotify took it. `value`: seconds for seek, 0/1 for shuffle and repeat, 0…100 for volume. */
  spotifyControl: (action: SpotifyAction, value?: number) =>
    call<boolean>("spotify_control", { action, value: value ?? null }),
  /** Brings Spotify forward or starts it; without it, its download page. */
  spotifyOpen: () => call<boolean>("spotify_open"),
  // ── Ciel's own updates (src-tauri/src/updates.rs) ─────────────────────────
  updateStatus: () => call<UpdateStatus>("update_status"),
  /** Asks the latest release whether a newer version is out. */
  updateCheck: () => call<UpdateStatus>("update_check"),
  /** Downloads, checks and installs it; Ciel restarts. Only on the user's press. */
  updateInstall: () => call<void>("update_install"),
  // ── YouTube Music, through Ciel for Chrome (src-tauri/src/ytmusic.rs) ─────
  /** The player in a YouTube Music tab, in Spotify's shape. */
  ytMusicState: () => call<SpotifyState>("ytmusic_state"),
  /** To that tab: what the media keys do; `seek` to `value` seconds; `focus` brings the tab forward. */
  ytMusicControl: (action: "playPause" | "next" | "previous" | "seek" | "focus", value?: number) =>
    call<void>("ytmusic_control", { action, value: value ?? null }),
  /** Whether the browsers know Ciel for Chrome's bridge, and the places involved (browser_bridge.rs). */
  browserBridgeStatus: () => call<{ installed: boolean; places: string[] }>("browser_bridge_status"),
  /** Makes the bridge known to the browsers, or takes it out. */
  browserBridgeSet: (on: boolean) => call<{ installed: boolean; places: string[] }>("browser_bridge_set", { on }),
  /** Ciel's GNOME Shell extension: whether it is of use, installed, running, and what it writes (gnome_ext.rs). */
  gnomeExtensionStatus: () => call<GnomeExtStatus>("gnome_extension_status"),
  /** Installs it (its files, GNOME's list of enabled extensions) or takes it out. */
  gnomeExtensionSet: (on: boolean) => callOrThrow<GnomeExtStatus>("gnome_extension_set", { on }),
  /** A notification's Open (back to its tab or its mail) or Dismiss (src-tauri/src/notify.rs). */
  notificationAction: (id: string, action: "open" | "dismiss") => call<void>("notification_action", { id, action }),
  /** Whether there is a Spotify to launch (Settings). */
  spotifyInstalled: () => call<boolean>("spotify_installed"),

  // ── Finance (src-tauri/src/finance.rs) ────────────────────────────────────
  /** The folder's summary if anything changed since `stamp`, else null. Two file stats. */
  financePoll: (stamp: string | null) => call<FinancePoll | null>("finance_poll", { stamp }),
  /** Adds an expense and answers with the fresh summary; throws the reason it was refused. */
  financeAddExpense: (expense: NewExpense) => callOrThrow<FinancePoll>("finance_add_expense", { expense }),
  /** What a folder holds (the saved one without `path`). */
  financeFolderInfo: (path?: string) => call<FinanceFolderInfo>("finance_folder_info", { path: path ?? null }),
  /** Shows the folder, making it with its defaults first if it is new. */
  financeOpenFolder: () => call<void>("finance_open_folder"),

  // ── Alerts (src-tauri/src/alerts.rs) ──────────────────────────────────────
  /** Where to send alerts, the token hidden, and whether Ciel listens. */
  alertsInfo: () => call<AlertsInfo>("alerts_info"),
  /** Copies the address with its token; the token never comes to the page. */
  alertsCopyAddress: () => call<boolean>("alerts_copy_address"),
  /** A new token, so the old address stops working; the new address is copied. */
  alertsNewAddress: () => call<boolean>("alerts_new_address"),
  /** Copies the script of a Zabbix media type of type Webhook. */
  alertsCopyZabbixScript: () => call<boolean>("alerts_copy_zabbix_script"),
  /** Takes an alert off the list, until its tool sends it again. */
  alertsDismiss: (key: string) => call<void>("alerts_dismiss", { key }),

  // ── Wallpapers (the Rust side of src/sky; island/wallpaper.ts drives it) ──
  /** The displays, in physical pixels; `id` is what wallpaperSetDesktop takes. */
  wallpaperMonitors: () => call<WallpaperMonitor[]>("wallpaper_monitors"),
  /**
   * A PNG for one display (Fill). The first change saves the user's own
   * wallpaper. The id travels percent-encoded in a header (a device path has
   * characters a header can't carry). Throws why it failed.
   */
  wallpaperSetDesktop: (monitorId: string, png: Uint8Array) =>
    sendBytes("wallpaper_set_desktop", png, { monitorId: encodeURIComponent(monitorId) }, { monitorId }),
  /** A PNG for the lock screen (Windows). The first change saves the user's own picture. */
  wallpaperSetLockScreen: (png: Uint8Array) => sendBytes("wallpaper_set_lock_screen", png, {}, {}),
  /** Puts back the wallpapers and lock screen saved before Ciel's first change; stops live mode. */
  wallpaperRestore: () => call<void>("wallpaper_restore"),
  /** The photos already downloaded for a theme. */
  wallpaperPhotos: (theme: string) => call<WallpaperPhoto[]>("wallpaper_photos", { theme }),
  /** Downloads free photos for a theme (only ever from a click); how many are new. */
  wallpaperPhotosFetch: (theme: string, count: number) => callOrThrow<number>("wallpaper_photos_fetch", { theme, count }),
  /** A cached photo's bytes, to decode here (the page can't load file URLs). */
  wallpaperPhotoBytes: async (path: string) => toBytes(await call<ArrayBuffer | number[]>("wallpaper_photo_bytes", { path })),
  /** Creates (or destroys) the live window(s) behind the desktop icons. */
  /** False when the live windows could not start (no layer-shell: GNOME, X11). */
  wallpaperLive: (enabled: boolean) =>
    callOrThrow<void>("wallpaper_live", { enabled }).then(
      () => true,
      () => false,
    ),
  /** What this OS supports, and whether live mode runs, is paused, and a wallpaper is saved. */
  wallpaperStatus: () => call<WallpaperStatus>("wallpaper_status"),
};

export interface WallpaperMonitor {
  /** The monitor's device path (Windows) or output name (Linux). */
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  primary: boolean;
}

export interface WallpaperPhoto {
  path: string;
  title: string;
  author: string;
  license: string;
  sourceUrl: string;
  width: number;
  height: number;
}

export interface WallpaperStatus {
  supported: { desktop: boolean; lockScreen: boolean; live: boolean };
  live: boolean;
  paused: boolean;
  saved: boolean;
}

/** Bytes from Rust: an ArrayBuffer (ipc::Response) or, from a plain Vec<u8>, numbers. */
function toBytes(v: ArrayBuffer | ArrayLike<number> | null): Uint8Array | null {
  if (!v) return null;
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  return Uint8Array.from(v);
}

/**
 * A command that takes a PNG of several MB: the bytes go as the raw request
 * body, the other arguments as `headers` (tauri::ipc::Request). Should a
 * build of the command want its arguments the ordinary way, they are sent
 * again as JSON (`args`, with `png` as a number array). Throws why it failed.
 */
async function sendBytes(cmd: string, png: Uint8Array, headers: Record<string, string>, args: Record<string, string>): Promise<void> {
  const routed = gate?.command(cmd, args);
  if (routed) {
    await routed.value;
    return;
  }
  if (!IS_TAURI) throw new Error("not running inside Ciel");
  try {
    await invoke(cmd, png, { headers });
  } catch (err) {
    if (!/invalid args|missing required key|expected a value/i.test(String(err))) throw err;
    await invoke(cmd, { ...args, png: Array.from(png) });
  }
}

export type SpotifyAction = "playPause" | "next" | "previous" | "seek" | "shuffle" | "repeat" | "volume";

/** The Alerts pill, as Settings shows it (src-tauri/src/alerts.rs AlertsInfo). */
export interface AlertsInfo {
  /** The address to give a monitoring tool, its token hidden. */
  address: string;
  listening: boolean;
  /** Why Ciel doesn't listen (the port is taken…), or null. */
  error: string | null;
}

/** Ciel's own updates, as Settings shows them (src-tauri/src/updates.rs UpdateStatus). */
export interface UpdateStatus {
  /** This build knows the key updates are signed with. */
  ready: boolean;
  current: string;
  /** A newer version found by the last check. */
  available: string | null;
  /** This install replaces itself (Windows, an AppImage); else the release page. */
  canInstall: boolean;
  busy: boolean;
  /** 0…1 while downloading, when the size is known. */
  progress: number | null;
  error: string | null;
  /** When the last check ended (Unix ms), 0 when none did. */
  checkedAt: number;
}

export type ShortcutStatus =
  | "active" | "off" | "inUse" | "duplicate" | "invalid"
  | "typesCharacter" | "unsupported" | "notPorted"
  /** Wayland: handed to the desktop's portal, no answer yet / left out by it. */
  | "pending" | "refused";

export interface ShortcutsReport {
  /** `trigger`: on Wayland, the keys the desktop says run it, in its words. */
  actions: { id: string; status: ShortcutStatus; typed?: string; trigger?: string }[];
  /** "wayland" or "no-display" when no key grab can be made. */
  blocked: string | null;
  /** `<executable> --shortcut`: append an action id for a desktop shortcut. */
  command: string;
  /** Wayland: the shortcuts went to the desktop's GlobalShortcuts portal. */
  portal?: "pending" | "active" | null;
}

/** How the desktop Ciel's window works here (platform::DesktopMode). */
export type DesktopMode = "poll" | "window" | "layer" | "off";

export interface DesktopInfo {
  mode: DesktopMode;
  /** He was on the desktop when the app last quit. */
  onDesktop: boolean;
}

/** An event for one window only (island ⇄ desktop Ciel). Never throws. */
export async function emitToWindow(label: string, event: string, payload?: unknown) {
  if (!IS_TAURI) return;
  try {
    await emitTo(label, event, payload);
  } catch (err) {
    console.error(`[ciel] emit ${event} failed`, err);
  }
}

/** An event for every window (the live wallpaper windows, whatever their number). Never throws. */
export async function broadcast(event: string, payload?: unknown) {
  if (!IS_TAURI) return;
  try {
    await emit(event, payload);
  } catch (err) {
    console.error(`[ciel] emit ${event} failed`, err);
  }
}

export interface IntegrationUpdate {
  id: string;
  data: Record<string, unknown>;
  error: string | null;
  event: { success: boolean; label: string; detail: string | null } | null;
}

export type ChatContext =
  | { kind: "file"; name: string; path: string }
  | { kind: "window"; appName: string; title: string; url?: string };

export interface ModelInfo {
  id: string;
  label: string;
}

export interface LocalServer {
  /** The address as it is stored. */
  url: string;
  models: string[];
  /** The address is this machine. */
  loopback: boolean;
}

export interface DroppedFile {
  name: string;
  path: string;
  size: number;
}

export interface HookStatus {
  installed: boolean;
  /** Ciel's status line relay (plan usage) is the status line in settings.json. */
  planRelayInstalled: boolean;
  settingsPath: string;
  hookPath: string;
  hookReady: boolean;
}

/** One agent other than Claude Code, as agents.rs reports it. */
export interface AgentHookStatus {
  /** The `--agent` name; its pill is `agent_<id>`. */
  id: string;
  name: string;
  installed: boolean;
  /** The file (or files, one per line) Ciel writes. */
  path: string;
  hookReady: boolean;
  /** The island can allow or deny this agent's permission requests. */
  approvals: boolean;
  /** What to do once it is written. */
  note: string;
}

export interface AgentHookPlan {
  diff: string;
  /** Where each existing file is copied first, one per line; "" when none. */
  backup: string;
  path: string;
  fingerprint: string;
}

export interface HookPreview {
  diff: string;
  backup: string;
  settingsPath: string;
  /** Hand back to hooksApply so only the reviewed diff is ever written. */
  fingerprint: string;
}

/** Same as `call`, but surfaces the error so the UI can show what went wrong. */
async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const routed = gate?.command(cmd, args);
  if (routed) return (await routed.value) as T;
  if (!IS_TAURI) throw new Error("not running inside Ciel");
  return invoke<T>(cmd, args);
}

export type BridgeEvent =
  | { name: "cursor"; payload: { x: number; y: number } }
  | { name: "tray"; payload: string }
  | { name: "hook"; payload: Record<string, unknown> }
  | { name: "screen-changed"; payload: null };

export interface DragDropPayload {
  type: "enter" | "over" | "drop" | "leave";
  paths?: string[];
}

interface WebView2Bridge {
  postMessageWithAdditionalObjects(message: unknown, objects: ArrayLike<unknown>): void;
}

/**
 * Files dragged onto the island. Only reaches us when the window takes the mouse.
 *
 * On Windows WebView2 takes the drop itself, as in Edge — Tauri's drop handling
 * never sees drags from the classic Explorer folder view (see
 * src-tauri/src/webview_drop.rs). Enter/over/leave come straight from the page;
 * the drop hands the File objects to Rust, which answers with their real paths
 * as a `file-drag` event. Elsewhere (Linux) Tauri's own drag events still do it.
 */
export async function onDragDrop(handler: (e: DragDropPayload) => void) {
  if (!IS_TAURI) return () => {};
  const webview = (window as unknown as { chrome?: { webview?: WebView2Bridge } }).chrome?.webview;
  if (!webview) {
    return getCurrentWebview().onDragDropEvent((event) => {
      handler(event.payload as DragDropPayload);
    });
  }
  const hasFiles = (e: DragEvent) => e.dataTransfer?.types.includes("Files") ?? false;
  // dragenter/dragleave fire for every element crossed; only the outermost pair counts.
  let depth = 0;

  const onEnter = (e: DragEvent) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (depth++ === 0) handler({ type: "enter" });
  };
  const onOver = (e: DragEvent) => {
    if (!hasFiles(e)) return;
    // Without this WebView2 refuses the drop — or navigates to the file.
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    handler({ type: "over" });
  };
  const onLeave = (e: DragEvent) => {
    if (!hasFiles(e) || depth === 0) return;
    if (--depth === 0) handler({ type: "leave" });
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    depth = 0;
    const files = e.dataTransfer?.files;
    if (!files || files.length === 0) {
      handler({ type: "drop", paths: [] });
      return;
    }
    webview.postMessageWithAdditionalObjects("ciel-file-drop", files);
  };

  window.addEventListener("dragenter", onEnter);
  window.addEventListener("dragover", onOver);
  window.addEventListener("dragleave", onLeave);
  window.addEventListener("drop", onDrop);
  const unlisten = await listen<DragDropPayload>("file-drag", (e) => handler(e.payload));
  return () => {
    window.removeEventListener("dragenter", onEnter);
    window.removeEventListener("dragover", onOver);
    window.removeEventListener("dragleave", onLeave);
    window.removeEventListener("drop", onDrop);
    unlisten();
  };
}

/** Every handler given to onEvent, by event, so the page can hand them an event itself. */
const pageHandlers = new Map<string, Set<(payload: unknown) => void>>();

export async function onEvent<T>(name: string, handler: (payload: T) => void) {
  const own = handler as (payload: unknown) => void;
  const handlers = pageHandlers.get(name) ?? new Set();
  pageHandlers.set(name, handlers);
  handlers.add(own);
  const forget = () => void handlers.delete(own);
  if (!IS_TAURI) return forget;
  const unlisten = await listen<T>(name, (e) => {
    if (gate && !gate.event(name, e.payload)) return;
    handler(e.payload);
  });
  return () => {
    forget();
    unlisten();
  };
}

/**
 * Hands an event to this page's own handlers as if Rust had sent it (the demo's
 * streamed chat answer, or what it held back while it ran). Never gated.
 */
export function emitLocal(name: string, payload?: unknown) {
  for (const handler of [...(pageHandlers.get(name) ?? [])]) handler(payload);
}

export interface GnomeExtStatus {
  available: boolean;
  installed: boolean;
  active: boolean;
  places: string[];
}
