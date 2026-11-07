// The Tools tab, the Today card and the live chip in a plain browser
// (`npx vite`, /dev/tools-preview.html), fed made-up timers, copies, files,
// controls and weather. Nothing reaches Rust: the Bridge is a no-op here.

import "../src/style.css";
import { State } from "../src/core/state";
import { Tools } from "../src/tools/store";
import { buildTools } from "../src/tools/view";
import { buildToday } from "../src/views/today";
import { buildLiveChip } from "../src/island/live";
import type { ViewActions } from "../src/views/views";
import type { ToolId } from "../src/core/tools";
import { setLanguage, isLanguage } from "../src/i18n/i18n";

const lang = new URLSearchParams(location.search).get("lang");
if (lang && isLanguage(lang)) setLanguage(lang);

const now = Date.now();
const MIN = 60_000;
State.mode = "expanded";
State.view = "tools";
State.settings.tools = { ...State.settings.tools, clipboard: true, weatherPlace: "Lyon, France", weatherLat: 45.75, weatherLon: 4.85, rainWarnings: true };

Tools.timers = [
  { id: "a", kind: "timer", label: "Tea", due: now + 3 * MIN + 12_000, duration: 4 * MIN, repeat: "", ringing: false, phase: "", round: 0, focusMs: 0, breakMs: 0 },
  { id: "b", kind: "focus", label: "", due: now + 17 * MIN, duration: 25 * MIN, repeat: "", ringing: false, phase: "focus", round: 1, focusMs: 25 * MIN, breakMs: 5 * MIN },
  { id: "c", kind: "reminder", label: "Call mum", due: now + 5 * 3_600_000, duration: 0, repeat: "weekly", ringing: false, phase: "", round: 0, focusMs: 0, breakMs: 0 },
];
const swatch = "data:image/svg+xml;utf8," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><defs><linearGradient id="g"><stop offset="0" stop-color="#7fb4ea"/><stop offset="1" stop-color="#f472b6"/></linearGradient></defs><rect width="80" height="60" fill="url(#g)"/></svg>');
Tools.clip = {
  on: true,
  watching: true,
  items: [
    { id: 1, kind: "text", text: "https://example.com/article?id=42&utm_source=newsletter&utm_medium=email&fbclid=abc", width: 0, height: 0, at: now - 40_000, pinned: false, app: "Chrome" },
    { id: 2, kind: "text", text: "#7FB4EA", width: 0, height: 0, at: now - 3 * MIN, pinned: false, app: "Figma" },
    { id: 3, kind: "text", text: "482910", width: 0, height: 0, at: now - 20_000, pinned: false, app: "Messages", expires: now + 100_000 },
    { id: 4, kind: "text", text: "1 200 / 3 + 5", width: 0, height: 0, at: now - 9 * MIN, pinned: false, app: "Notepad" },
    { id: 5, kind: "image", text: "", thumb: swatch, width: 1920, height: 1080, at: now - 14 * MIN, pinned: false, app: "Snipping Tool" },
    { id: 6, kind: "text", text: "Thanks for the update — I'll send the signed contract back on Monday morning, after the meeting with legal.", width: 0, height: 0, at: now - 50 * MIN, pinned: true, app: "Outlook" },
  ],
};
Tools.shelf = [
  { name: "invoice-october.pdf", path: "C:/Users/me/AppData/Local/Ciel/inbox/invoice-october.pdf", size: 182_000, at: now - 2 * 3_600_000, preview: null },
  { name: "sky.png", path: "C:/Users/me/AppData/Local/Ciel/inbox/sky.png", size: 2_400_000, at: now - 3_600_000, preview: swatch },
  { name: "notes.md", path: "C:/Users/me/AppData/Local/Ciel/inbox/notes.md", size: 4_200, at: now - 30 * MIN, preview: null },
];
Tools.controls = { volume: 0.46, muted: false, micMuted: false, mics: 2, brightness: 0.7, wifi: true, bluetooth: false, nightLight: null, awake: true, awakeUntil: now + 54 * MIN };
const hour = (i: number) => ({ t: now + i * 3_600_000, temp: 12 + Math.sin(i / 3) * 4, code: i > 2 && i < 6 ? 61 : i > 7 ? 2 : 3, pop: i > 2 && i < 6 ? 80 : 10, isDay: i < 6 });
Tools.weather = {
  place: "Lyon, France", lat: 45.75, lon: 4.85, at: now - 8 * MIN,
  now: { temp: 12.4, feels: 10.2, code: 3, isDay: true, wind: 14, humidity: 72 },
  hours: Array.from({ length: 24 }, (_, i) => hour(i)),
  days: Array.from({ length: 7 }, (_, i) => ({ t: now + i * 86_400_000, code: [3, 61, 80, 2, 0, 71, 95][i], max: 14 - i * 0.5, min: 6 - (i % 3), pop: [10, 80, 70, 20, 0, 40, 60][i], sunrise: now - 3_600_000, sunset: now + 5 * 3_600_000 })),
  rainAt: now + 22 * MIN, raining: false, rainUntil: null,
};

const actions = {
  setView: (v: string) => console.log("setView", v),
  collapse: () => console.log("collapse"),
  blip: () => {},
} as unknown as ViewActions;

const root = document.getElementById("cases")!;
function show(label: string, height: number, el: HTMLElement, bot = { x: 46, y: 32, d: 38 }) {
  const c = document.createElement("div");
  c.className = "case";
  const box = document.createElement("div");
  box.className = "box";
  box.style.height = `${height}px`;
  const ciel = document.createElement("i");
  ciel.className = "ciel";
  Object.assign(ciel.style, { left: `${bot.x - bot.d / 2}px`, top: `${bot.y - bot.d / 2}px`, width: `${bot.d}px`, height: `${bot.d}px` });
  el.classList.add("on");
  box.append(el, ciel);
  c.append(Object.assign(document.createElement("span"), { textContent: label }), box);
  root.append(c);
  return el;
}

const tools = buildTools(actions);
show("Tools", 216, tools.el);
const today = buildToday(actions);
show("Today (nothing running)", 118, today.el, { x: 70, y: 59, d: 62 });

const tabs: ToolId[] = ["timers", "clipboard", "shelf", "controls", "weather"];
const bar = document.getElementById("tabs")!;
for (const id of tabs) {
  const b = document.createElement("button");
  b.textContent = id;
  b.onclick = () => {
    Tools.tab = id;
    State.notify();
  };
  bar.append(b);
}
const initial = new URLSearchParams(location.search).get("tab");
if (initial && tabs.includes(initial as ToolId)) Tools.tab = initial as ToolId;

const compact = document.getElementById("compact")!;
const chip = buildLiveChip((tool) => console.log("open", tool));
compact.append(chip.el);

function frame(t: number) {
  tools.tick?.(t);
  requestAnimationFrame(frame);
}
State.subscribe(() => {
  tools.sync();
  today.sync();
  chip.sync(true);
});
State.notify();
requestAnimationFrame(frame);
