// The System pill's card in a plain browser (`npx vite`, /dev/system-preview.html),
// fed a busy computer: memory at warning, the processor critical.

import "../src/style.css";
import { State } from "../src/core/state";
import { SYSTEM_ID, renderSystem } from "../src/views/system";

const wave = (base: number, n = 40) => Array.from({ length: n }, (_, i) => base + Math.sin(i / 4) * 8 + (i / n) * 6);
State.integrations[SYSTEM_ID] = {
  loaded: true,
  error: null,
  configured: true,
  data: {
    cpu: 42, ram: 84, disk: 61, temp: 96,
    ramUsed: 27.9e9, ramTotal: 33.2e9, diskUsed: 300e9, diskTotal: 490e9, diskMount: "/",
    levels: { cpu: "ok", ram: "warning", disk: "ok", temp: "critical" },
    history: { cpu: wave(36), ram: wave(78), disk: wave(60, 40).map(() => 61), temp: wave(88) },
    top: [
      { pid: 1, name: "cargo", cpu: 38.2, memory: 1.9e9, own: true },
      { pid: 2, name: "chrome", cpu: 6.1, memory: 4.1e9, own: true },
      { pid: 3, name: "Xorg", cpu: 2.4, memory: 241e6, own: false },
      { pid: 4, name: "spotify", cpu: 0.9, memory: 438e6, own: true },
      { pid: 5, name: "gnome-shell", cpu: 0.6, memory: 512e6, own: true },
    ],
  },
};

const root = document.getElementById("cases")!;
const show = (label: string, width: number, card: HTMLElement) => {
  const box = document.createElement("div");
  box.className = "box";
  box.style.width = `${width}px`;
  const ciel = document.createElement("i");
  ciel.className = "ciel";
  box.append(ciel, card);
  const c = document.createElement("div");
  c.className = "case";
  c.append(Object.assign(document.createElement("span"), { textContent: label }), box);
  root.append(c);
};
show("Narrow: the rings, Processes behind", 440, renderSystem(false, false, () => {}, () => {}));
show("Narrow, Processes open", 440, renderSystem(false, true, () => {}, () => {}));
show("Wide: the rings and the processes side by side", 640, renderSystem(true, false, () => {}, () => {}));
