// The screensaver (screensaver.html), on the main display only: a flip clock
// on pitch black, like Fliqlo, in the size chosen in Settings, the date under
// it if wanted. Rust makes the window at launch, shows it after the minutes
// chosen and takes it away again (src-tauri/src/screensaver.rs); a key, a
// click or the mouse moving sends it away from here. Nothing moves but the
// cards, once a minute, and nothing at all while it's hidden.

import "./screensaver.css";
import { Bridge, onEvent } from "../core/bridge";
import type { Settings } from "../core/state";
import { buildFlipClock, setClockFormat, tick } from "../views/flipclock";

/** The pointer may tremble: it has to travel this far to wake the screen. */
const MOVE_PX = 14;
/** Keys that wake nothing alone: Windows presses Alt itself as the clock comes to the front. */
const MODIFIERS = new Set(["Alt", "AltGraph", "Control", "Shift", "Meta"]);

const flip = buildFlipClock("saver-flip");
const date = document.createElement("div");
date.className = "saver-date";
const face = document.createElement("div");
face.className = "saver-face";
face.append(flip.el, date);
document.body.append(face);

function apply(s: Partial<Settings> | null | undefined) {
  if (!s) return;
  if (s.language) document.documentElement.lang = s.language;
  setClockFormat(s.clockFormat);
  document.body.dataset.size = s.screensaverSize ?? "large";
  date.hidden = s.screensaverDate === false;
}

function refreshDate() {
  date.textContent = new Date().toLocaleDateString(document.documentElement.lang || undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
}

// ── Showing and going away ────────────────────────────────────────────────────

// Input counts only while the clock is up: from "screensaver-shown", which Rust
// sends before the window appears, until it goes. What comes outside that is
// ignored; Rust sees the user back from the idle time anyway.
let up = false;
let shownAt = 0;
let from: { x: number; y: number } | null = null;
let stopTick: (() => void) | null = null;
let dateTimer = 0;

function leave() {
  // The pointer under the window as it appears is not a move.
  if (!up || performance.now() - shownAt < 700) return;
  up = false;
  void Bridge.screensaverDismiss();
}
window.addEventListener("keydown", (e) => {
  if (!MODIFIERS.has(e.key)) leave();
});
window.addEventListener("mousedown", leave);
window.addEventListener("wheel", leave, { passive: true });
window.addEventListener("touchstart", leave, { passive: true });
window.addEventListener("mousemove", (e) => {
  if (!from) from = { x: e.screenX, y: e.screenY };
  else if (Math.hypot(e.screenX - from.x, e.screenY - from.y) > MOVE_PX) leave();
});

void onEvent<boolean>("screensaver-shown", () => {
  up = true;
  shownAt = performance.now();
  from = null;
  stopTick?.();
  stopTick = tick(flip);
  refreshDate();
  window.clearInterval(dateTimer);
  // The date changes at midnight: once a minute is plenty.
  dateTimer = window.setInterval(refreshDate, 60_000);
});
void onEvent<boolean>("screensaver-hidden", () => {
  up = false;
  from = null;
  stopTick?.();
  stopTick = null;
  window.clearInterval(dateTimer);
});
void onEvent<Settings>("settings-changed", apply);

void Bridge.boot()
  .then((boot) => apply(boot?.settings))
  .catch(() => {})
  .finally(() => {
    // Never an empty card, should the first "shown" come before the page.
    flip.set(new Date());
    refreshDate();
  });
