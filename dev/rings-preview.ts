// The folded island's plan rings, in a plain browser (`npx vite`, /dev/rings-preview.html):
// a few readings side by side, and the hover card open on the last one.

import "../src/style.css";
import { DEFAULT_SETTINGS, State } from "../src/core/state";
import { buildPlanRings } from "../src/views/plan-rings";

const H = 3_600_000;
const now = Date.now();
const win = (usedPct: number, inH: number) => ({ usedPct, resetsAt: now + inH * H });

interface Case {
  label: string;
  claude: typeof State.planUsage;
  codex: typeof State.codexPlanUsage;
  others?: boolean;
  pop?: "claude" | "codex";
}

const CASES: Case[] = [
  { label: "Claude 34 %, Codex 22 %", claude: { fiveHour: win(34, 2), sevenDay: win(21, 72), updatedAt: now }, codex: { sevenDay: win(22, 120), updatedAt: now } },
  { label: "Orange and red, beside other sessions", claude: { fiveHour: win(62, 2), sevenDay: win(48, 72), updatedAt: now }, codex: { fiveHour: win(91, 1), sevenDay: win(40, 120), updatedAt: now }, others: true },
  { label: "Over the limit; Codex not answered yet", claude: { fiveHour: win(100, 1), updatedAt: now }, codex: null },
  { label: "Old readings, dimmed", claude: { fiveHour: win(12, 2), sevenDay: win(30, 72), updatedAt: now - 5 * H }, codex: { sevenDay: win(8, 120), updatedAt: now - H } },
  { label: "Hovered: the card", claude: { fiveHour: win(34, 2.2), sevenDay: win(61, 72), updatedAt: now - 3 * 60_000 }, codex: { fiveHour: win(9, 3), sevenDay: win(22, 120), planType: "plus", resetCredits: 0, updatedAt: now }, pop: "claude" },
];

State.settings = { ...DEFAULT_SETTINGS, showPlanInNotch: true, planRelayInstalled: true, showCodexPlanInNotch: true };
State.mode = "compact";

const root = document.getElementById("cases")!;
for (const c of CASES) {
  State.planUsage = c.claude;
  State.codexPlanUsage = c.codex;
  const rings = buildPlanRings();
  const strip = document.createElement("div");
  strip.className = "strip";
  const ciel = document.createElement("i");
  ciel.className = "ciel";
  strip.append(ciel, rings.el, rings.pop);
  rings.el.style.right = c.others ? "62px" : "14px";
  const box = document.createElement("div");
  box.className = "case";
  box.append(Object.assign(document.createElement("span"), { textContent: c.label }), strip);
  root.append(box);
  rings.sync();
  if (c.pop) {
    root.append(Object.assign(document.createElement("div"), { className: "gap" }));
    const ring = rings.el.querySelector<HTMLElement>(`.plan-ring-${c.pop}`)!;
    ring.dispatchEvent(new MouseEvent("mouseenter"));
  }
}
