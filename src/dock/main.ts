// The dock (dock.html): a bar at the bottom of the screen, as a desktop's
// own dock has it (drawn in bar.ts). Rust places the window, the bar's size
// with room around it for the icons that grow, and sends the row and the
// corner's icons (src-tauri/src/dock); the island sends the agents and the
// music (src/island/dock.ts).
//
// It never takes the keyboard. While it is off or out of a full-screen app's
// way, nothing runs here: the clock waits for the dock to be back, and the
// icons move only while the pointer is on them.

import "./dock.css";
import { Bridge, emitToWindow, onEvent } from "../core/bridge";
import { DOCK_EVENTS, untilNextMinute, type DockSnapshot, type DockState, type SystemIcons } from "../core/dock";
import { DEFAULT_SETTINGS, type Settings } from "../core/state";
import { applyTheme, onWhite } from "../core/theme";
import { onLanguageChange, resolveLanguage, setLanguage, systemLanguages } from "../i18n/i18n";
import { buildDockBar } from "./bar";

const ISLAND = "island";

/** The pointer is on the bar: the window's room takes clicks too (Rust's window region). */
let hover = false;

const bar = buildDockBar({
  start: () => void Bridge.dockStart(),
  panel: (what) => void Bridge.dockKey(what),
  agents: (waiting) => void emitToWindow(ISLAND, DOCK_EVENTS.open, waiting ? "waiting" : "agents"),
  open: (id, newWindow) => void Bridge.dockOpen(id, newWindow),
  menu: (id) => void Bridge.dockMenu(id),
  preview: (id, x) => void Bridge.dockPreview(id, x, onWhite()),
  pinMove: (id, to) => void Bridge.dockPinMove(id, to),
  volume: (notches) => void Bridge.dockVolume(notches),
  playPause: () => void Bridge.spotifyControl("playPause"),
  taskbar: () => void Bridge.dockTaskbar(),
  hover: (on) => {
    hover = on;
    void Bridge.dockRegion(on);
  },
});
document.body.append(bar.root);

// The window takes the bar's size at rest (Rust centres it at the bottom of
// the screen); the icons growing never change it.
new ResizeObserver(() => void Bridge.dockFit(Math.ceil(bar.el.offsetWidth))).observe(bar.el);
// The window's new size: what takes clicks in it again.
window.addEventListener("resize", () => void Bridge.dockRegion(hover));
// No web page menu: a right click is the dock's own (bar.ts).
window.addEventListener("contextmenu", (e) => e.preventDefault());

// ── The clock: once a minute, while the dock is on screen ────────────────────

let clockTimer = 0;
let shown = false;

function runClock(on: boolean) {
  window.clearTimeout(clockTimer);
  clockTimer = 0;
  if (!on) return;
  const next = () => {
    bar.tick();
    clockTimer = window.setTimeout(next, untilNextMinute(Date.now()));
  };
  next();
}

// ── Settings, Rust and the island ─────────────────────────────────────────────

function apply(s: Partial<Settings> | null | undefined) {
  if (!s) return;
  setLanguage(resolveLanguage(s.language, systemLanguages()));
  applyTheme(s.islandTheme ?? "black");
  bar.setClockFormat(s.clockFormat ?? "auto");
  const dock = { ...DEFAULT_SETTINGS.dock, ...s.dock };
  bar.setLook({ magnify: dock.magnify, taskView: dock.taskView, widgets: dock.widgets });
}

/** The row, and the dock on screen or not. */
function take(s: DockState) {
  bar.setState(s);
  if (s.shown !== shown) {
    shown = s.shown;
    runClock(shown);
    // Back on screen: the island says again what it knows.
    if (shown) void emitToWindow(ISLAND, DOCK_EVENTS.ready);
  }
}

onLanguageChange(() => bar.relabel());
void onEvent<DockState>(DOCK_EVENTS.state, take);
void onEvent<SystemIcons>(DOCK_EVENTS.system, (s) => bar.setSystem(s));
void onEvent<DockSnapshot>(DOCK_EVENTS.island, (s) => bar.setSnapshot(s));
void onEvent<Settings>("settings-changed", apply);

void Bridge.boot()
  .then((boot) => apply(boot?.settings))
  .catch(() => {})
  .finally(() => {
    void Bridge.dockRegion(false);
    void Bridge.dockState().then((s) => s && take(s));
    void emitToWindow(ISLAND, DOCK_EVENTS.ready);
  });
