// The screensaver (screensaver.html), on the main display only: a flip clock
// on pitch black, like Fliqlo, in the size chosen in Settings, the date under
// it if wanted. Rust shows it after the minutes chosen and takes it away again
// (src-tauri/src/screensaver.rs); a key, a click or the mouse moving sends it
// away from here. Nothing moves but the cards, once a minute.

import "./screensaver.css";
import { Bridge, onEvent } from "../core/bridge";
import type { Settings } from "../core/state";
import { buildFlipClock, setClockFormat, tick } from "../views/flipclock";

/** The pointer may tremble: it has to travel this far to wake the screen. */
const MOVE_PX = 14;

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

// ── Going away ────────────────────────────────────────────────────────────────

let shownAt = performance.now();
let from: { x: number; y: number } | null = null;
let leaving = false;
function leave() {
  // The pointer under the window as it appears is not a move.
  if (leaving || performance.now() - shownAt < 700) return;
  leaving = true;
  void Bridge.screensaverDismiss();
}
window.addEventListener("keydown", leave);
window.addEventListener("mousedown", leave);
window.addEventListener("wheel", leave, { passive: true });
window.addEventListener("touchstart", leave, { passive: true });
window.addEventListener("mousemove", (e) => {
  if (!from) from = { x: e.screenX, y: e.screenY };
  else if (Math.hypot(e.screenX - from.x, e.screenY - from.y) > MOVE_PX) leave();
});

void onEvent<boolean>("screensaver-shown", () => {
  shownAt = performance.now();
  from = null;
  leaving = false;
  flip.set(new Date());
  refreshDate();
});
void onEvent<Settings>("settings-changed", apply);

void Bridge.boot()
  .then((boot) => apply(boot?.settings))
  .catch(() => {})
  .finally(() => {
    tick(flip);
    refreshDate();
    // The date changes at midnight: once a minute is plenty.
    window.setInterval(refreshDate, 60_000);
  });
