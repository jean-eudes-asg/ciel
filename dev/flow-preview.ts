// Dictation's looks in the folded island, in a plain browser (`npx vite`,
// /dev/flow-preview.html): each case feeds the real strip a state, and the
// voice moves the dots.

import "../src/style.css";
import { buildFlowStrip, type FlowSource } from "../src/island/flow";
import type { PillState } from "../src/core/flow";

const base: PillState = {
  phase: "idle", edge: "island", key: "right_ctrl", noteStartedAt: null, announcement: null, enabled: true, voice: null,
};

const CASES: [string, Partial<PillState>, boolean?][] = [
  ["Dictating: the dots follow your voice", { phase: "listening" }],
  ["Hands-free: ✕, the dots, ✓", { phase: "locked" }],
  ["Thinking: the words are on their way", { phase: "thinking" }],
  ["A short message", { phase: "copied" }],
  ["Recording a call", { noteStartedAt: Date.now() - 754_000 }],
  ["« OK Ciel » answers", { voice: { phase: "speaking", heard: "What's left on my Claude plan this week?", answer: "You've used 61% of your week; it resets Thursday at 13:37. The 5-hour window is at 34%." } }, true],
  ["A message with its button", { announcement: { id: 1, icon: "download", tint: "#7FB4EA", text: "The speech model is ready", action: "Try it", actionId: "openSettings" } }, true],
];

const root = document.getElementById("cases")!;
for (const [label, over, tall] of CASES) {
  const listeners: ((b: number[]) => void)[] = [];
  const source: FlowSource = {
    onState: () => {},
    onBands: (fn) => listeners.push(fn),
    initial: async () => ({ ...base, ...over }),
  };
  const flow = buildFlowStrip(() => {}, source);
  const strip = document.createElement("div");
  strip.className = "strip";
  const ciel = document.createElement("i");
  ciel.className = "ciel";
  strip.append(ciel, flow.el, flow.pop);
  const box = document.createElement("div");
  box.className = tall ? "case tall" : "case";
  box.append(Object.assign(document.createElement("span"), { textContent: label }), strip);
  root.append(box);
  flow.sync(true);
  // A voice: the dots rise and fall.
  setInterval(() => {
    const t = performance.now() / 1000;
    listeners.forEach((fn) => fn(Array.from({ length: 9 }, (_, i) => 0.45 + 0.4 * Math.sin(t * 6 + i * 0.9) * Math.sin(t * 2.3 - i * 0.4))));
  }, 33);
}
