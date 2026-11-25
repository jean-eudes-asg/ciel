// The dock's bar in a plain browser (`npx vite`, /dev/dock-preview.html), fed
// made-up apps, agents and music. Nothing reaches Rust. The apps' icons are
// plain squares drawn here: no app's real icon.

import "../src/dock/dock.css";
import { buildDockBar } from "../src/dock/bar";
import type { DockApp, DockSnapshot, DockState, SystemIcons } from "../src/core/dock";
import { isLanguage, setLanguage } from "../src/i18n/i18n";

const lang = new URLSearchParams(location.search).get("lang");
if (lang && isLanguage(lang)) setLanguage(lang);

/** A square with a letter, standing in for an app's icon. */
function icon(letter: string, color: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect x="2" y="2" width="44" height="44" rx="11" fill="${color}"/><text x="24" y="32" font-family="Segoe UI, sans-serif" font-size="22" font-weight="700" fill="#fff" text-anchor="middle">${letter}</text></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

const app = (name: string, color: string | null, over: Partial<DockApp> = {}): DockApp => ({
  id: name, name, icon: color ? icon(name[0], color) : null, pinned: true, windows: 0, active: false, attention: false, ...over,
});

const ROW: DockApp[] = [
  app("Files", "#c9a227", { windows: 1 }),
  app("Code", "#2f6fdc", { windows: 2, active: true }),
  app("Terminal", "#2b2b2b", { windows: 1 }),
  app("Browser", "#d2452f"),
  app("Mail", "#1f8f6a"),
  app("Notes", null, { pinned: false, windows: 1 }),
];

const LAPTOP: SystemIcons = { volume: 0.62, muted: false, network: "wifi", networkName: "Home", battery: 71, charging: false };
const DESK: SystemIcons = { volume: 0.3, muted: true, network: "wired", networkName: "", battery: null, charging: false };

const cases: { title: string; state: DockState; snap: DockSnapshot; white?: boolean }[] = [
  {
    title: "Two agents at work, music, the taskbar hidden",
    state: { apps: ROW, taskbarHidden: true, shown: true, system: LAPTOP },
    snap: { agents: { asking: 0, working: 2, done: 1, error: 0 }, asker: null, playing: { title: "Golden Hour Drive", artist: "Nova Lane", playing: true } },
  },
  {
    title: "An agent asks you, an app flashes",
    state: { apps: ROW.map((a) => (a.name === "Mail" ? { ...a, windows: 1, attention: true } : a)), taskbarHidden: false, shown: true, system: DESK },
    snap: { agents: { asking: 1, working: 1, done: 0, error: 0 }, asker: "Claude Code", playing: { title: "Late Signal", artist: "Harbour Static", playing: false } },
  },
  {
    title: "Quiet: no agent, nothing playing",
    state: { apps: ROW.slice(0, 4).map((a) => ({ ...a, windows: 0, active: false })), taskbarHidden: false, shown: true, system: { ...LAPTOP, network: "", charging: true, battery: 18 } },
    snap: { agents: { asking: 0, working: 0, done: 0, error: 0 }, asker: null, playing: null },
  },
  {
    title: "On white (the white island)",
    state: { apps: ROW, taskbarHidden: true, shown: true, system: LAPTOP },
    snap: { agents: { asking: 0, working: 3, done: 0, error: 0 }, asker: null, playing: { title: "Golden Hour Drive", artist: "Nova Lane", playing: true } },
    white: true,
  },
];

const root = document.getElementById("cases")!;
for (const c of cases) {
  const log = (what: string) => (...args: unknown[]) => console.log("dock:", what, ...args);
  const bar = buildDockBar({
    start: log("start"), panel: log("panel"), agents: log("agents"), open: log("open"), menu: log("menu"),
    preview: log("preview"), pinMove: log("pinMove"), volume: log("volume"), playPause: log("playPause"),
    taskbar: log("taskbar"), hover: log("hover"),
  });
  bar.setState(c.state);
  bar.setSnapshot(c.snap);
  const slot = document.createElement("div");
  slot.className = c.white ? "slot theme-white" : "slot";
  slot.append(bar.root);
  const box = document.createElement("div");
  box.className = "case";
  const label = document.createElement("div");
  label.textContent = c.title;
  box.append(label, slot);
  root.append(box);
}
