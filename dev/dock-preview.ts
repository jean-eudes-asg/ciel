// The dock's bar in a plain browser (`npx vite`, /dev/dock-preview.html), fed
// made-up apps, agents and music. Nothing reaches Rust. The apps' icons are
// plain squares drawn here: no app's real icon.

import "../src/dock/dock.css";
import { buildDockBar } from "../src/dock/bar";
import type { CrewMate, DockApp, DockPlaying, DockSnapshot, DockState, SystemIcons } from "../src/core/dock";
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

const mate = (key: string, agent: string, project: string, line: string, phase: CrewMate["phase"]): CrewMate =>
  ({ key, agent, project, line, phase, since: Date.now() - 20_000 });

const LAPTOP: SystemIcons = { volume: 0.62, muted: false, network: "wifi", networkName: "Home", battery: 71, charging: false };
const DESK: SystemIcons = { volume: 0.3, muted: true, network: "wired", networkName: "", battery: null, charging: false };

/** A song 70 s in, with words a line every 4 s. */
const song = (title: string, artist: string, playing: boolean): DockPlaying => ({
  title, artist, playing, source: "spotify", opens: true, position: 70, positionAt: Date.now(), duration: 214,
  lyrics: Array.from({ length: 40 }, (_, i) => ({ t: 60 + i * 4, text: ["Headlights on the water", "We were never in a hurry", "Hold the gold a little longer", "Till the radio runs out"][i % 4] })),
});

const cases: { title: string; state: DockState; snap: DockSnapshot; white?: boolean }[] = [
  {
    title: "Two agents at work, music, the taskbar hidden",
    state: { apps: ROW, taskbarHidden: true, shown: true, system: LAPTOP },
    snap: { agents: { asking: 0, working: 2, done: 1, error: 0 }, asker: null, playing: song("Golden Hour Drive", "Nova Lane", true),
      plan: { fiveHour: { usedPct: 38, resetsAt: Date.now() + 3_600_000 }, sevenDay: { usedPct: 19, resetsAt: Date.now() + 86_400_000 }, updatedAt: Date.now() },
      crew: [mate("a", "Claude Code", "ciel", "Editing linux.rs", "working"), mate("b", "Codex", "site", "Running npm test", "working"), mate("c", "Gemini CLI", "notes", "Done", "done")] },
  },
  {
    title: "An agent asks you, an app flashes",
    state: { apps: ROW.map((a) => (a.name === "Mail" ? { ...a, windows: 1, attention: true } : a)), taskbarHidden: false, shown: true, system: DESK },
    snap: { agents: { asking: 1, working: 1, done: 0, error: 0 }, asker: "Claude Code", playing: song("Late Signal", "Harbour Static", false),
      plan: { fiveHour: { usedPct: 64, resetsAt: Date.now() + 3_600_000 }, sevenDay: { usedPct: 32, resetsAt: Date.now() + 86_400_000 }, updatedAt: Date.now() },
      crew: [mate("a", "Claude Code", "ciel", "", "asking"), mate("b", "Codex", "site", "Reading README.md", "working")] },
  },
  {
    title: "Quiet: no agent, nothing playing",
    state: { apps: ROW.slice(0, 4).map((a) => ({ ...a, windows: 0, active: false })), taskbarHidden: false, shown: true, system: { ...LAPTOP, network: "", charging: true, battery: 18 } },
    snap: { agents: { asking: 0, working: 0, done: 0, error: 0 }, asker: null, playing: null, crew: [], plan: null },
  },
  {
    title: "Just finished, one failed",
    state: { apps: ROW.slice(0, 4), taskbarHidden: false, shown: true, system: DESK },
    snap: { agents: { asking: 0, working: 0, done: 1, error: 1 }, asker: null, playing: null,
      plan: { fiveHour: { usedPct: 86, resetsAt: Date.now() + 3_600_000 }, sevenDay: { usedPct: 43, resetsAt: Date.now() + 86_400_000 }, updatedAt: Date.now() },
      crew: [mate("a", "Claude Code", "ciel", "All done", "done"), mate("b", "Codex", "site", "Failed", "error")] },
  },
  {
    title: "On white (the white island)",
    state: { apps: ROW, taskbarHidden: true, shown: true, system: LAPTOP },
    snap: { agents: { asking: 0, working: 5, done: 0, error: 0 }, asker: null, playing: song("Golden Hour Drive", "Nova Lane", true),
      plan: { fiveHour: { usedPct: 12, resetsAt: Date.now() + 3_600_000 }, sevenDay: { usedPct: 6, resetsAt: Date.now() + 86_400_000 }, updatedAt: Date.now() },
      crew: ["a", "b", "c", "d", "e"].map((k) => mate(k, "Claude Code", ["ciel", "site", "notes", "api", "docs"][k.charCodeAt(0) - 97], "Searching the code", "working")) },
    white: true,
  },
];

const root = document.getElementById("cases")!;
for (const c of cases) {
  const log = (what: string) => (...args: unknown[]) => console.log("dock:", what, ...args);
  const bar = buildDockBar({
    start: log("start"), panel: log("panel"), agents: log("agents"), hoverAgents: log("hoverAgents"), open: log("open"), menu: log("menu"),
    preview: log("preview"), pinMove: log("pinMove"), volume: log("volume"), music: log("music"),
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
