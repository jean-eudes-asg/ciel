// The System pill's card (src-tauri/src/system.rs): this computer's CPU,
// memory, disk and temperature as rings, each with its last two minutes, in
// green, orange or red at SysDash's limits; and the processes that weigh most,
// an End on the user's own, which ends one on its second click. Beside it when
// the island has room, behind "Processes" otherwise.

import { h, svg, dot } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { Bridge } from "../core/bridge";
import { t } from "../i18n/i18n";

export const SYSTEM_ID = "integration_system";
const COLORS = { ok: "#22C55E", warning: "#F5A524", critical: "#F4505E" } as const;
const NS = "http://www.w3.org/2000/svg";
/** An End clicked once waits this long for the second click. */
const ARMED_MS = 4000;

type Level = keyof typeof COLORS;

interface Process {
  pid: number;
  name: string;
  cpu: number;
  memory: number;
  own: boolean;
}

function data(): Record<string, unknown> {
  return (State.integrations[SYSTEM_ID]?.data ?? {}) as Record<string, unknown>;
}

export function hasSystemData(): boolean {
  return typeof data().cpu === "number";
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  return `${Math.round(n / 1e6)} MB`;
}

function el<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>): SVGElementTagNameMap[K] {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

/** A ring as full as `pct`, its value in the middle, its last minutes under it. */
function gauge(label: string, value: string, pct: number | null, level: Level, history: number[], max = 100): HTMLElement {
  const color = COLORS[level];
  const r = 17;
  const arc = 2 * Math.PI * r;
  const ring = el("svg", { viewBox: "0 0 44 44", width: 44, height: 44, class: "sys-ring" });
  ring.append(
    el("circle", { cx: 22, cy: 22, r, fill: "none", stroke: `${color}2e`, "stroke-width": 4 }),
    el("circle", {
      cx: 22, cy: 22, r, fill: "none", stroke: color, "stroke-width": 4, "stroke-linecap": "round",
      "stroke-dasharray": arc,
      "stroke-dashoffset": arc * (1 - Math.min(1, Math.max(0, (pct ?? 0) / max))),
      transform: "rotate(-90 22 22)",
    }),
  );
  const spark = el("svg", { viewBox: "0 0 60 14", width: 60, height: 14, class: "sys-spark", preserveAspectRatio: "none" });
  if (history.length > 1) {
    const step = 60 / (history.length - 1);
    const points = history.map((v, i) => `${(i * step).toFixed(1)},${(13 - (Math.min(max, v) / max) * 12).toFixed(1)}`).join(" ");
    spark.append(el("polyline", { points, fill: "none", stroke: color, "stroke-width": 1.4, "stroke-linejoin": "round" }));
  }
  return h("div", { class: `sys-gauge ${level}` },
    h("div", { class: "sys-dial" }, ring, h("b", { text: value })),
    h("span", { class: "sys-label", text: label }),
    spark,
  );
}

function levelOf(key: string): Level {
  const v = (data().levels as Record<string, unknown> | undefined)?.[key];
  return v === "warning" || v === "critical" ? v : "ok";
}

function series(key: string): number[] {
  const v = (data().history as Record<string, unknown> | undefined)?.[key];
  return Array.isArray(v) ? v.filter((x): x is number => typeof x === "number") : [];
}

function gauges(): HTMLElement {
  const d = data();
  const pct = (v: unknown) => (typeof v === "number" ? v : null);
  const row = h("div", { class: "sys-gauges" },
    gauge(t("CPU"), `${Math.round(pct(d.cpu) ?? 0)}%`, pct(d.cpu), levelOf("cpu"), series("cpu")),
    gauge(t("Memory"), `${Math.round(pct(d.ram) ?? 0)}%`, pct(d.ram), levelOf("ram"), series("ram")),
    gauge(t("Disk"), `${Math.round(pct(d.disk) ?? 0)}%`, pct(d.disk), levelOf("disk"), series("disk")),
  );
  // No sensor (a virtual machine, Windows without rights): no ring rather than an empty one.
  if (typeof d.temp === "number") {
    row.append(gauge(t("Temp"), `${Math.round(d.temp)}°`, d.temp, levelOf("temp"), series("temp"), 110));
  }
  return row;
}

function worst(): Level {
  const order: Level[] = ["ok", "warning", "critical"];
  return ["cpu", "ram", "disk", "temp"].map(levelOf).reduce((a, b) => (order.indexOf(b) > order.indexOf(a) ? b : a), "ok");
}

function systemCard(onDetail: (() => void) | null): HTMLElement {
  const d = data();
  const sub = typeof d.ramTotal === "number" ? `${bytes(Number(d.ramUsed))} / ${bytes(d.ramTotal)}` : "";
  const extra = onDetail
    ? h("button", { class: "int-more sys-more", title: t("Processes"), onclick: onDetail }, h("span", { text: t("Processes") }), svg(ICONS.chevronRight, 8, { stroke: 2.4 }))
    : h("span", { class: "int-total", text: sub });
  return h("div", { class: "int-card sys-card" }, header(extra), gauges());
}

function header(extra: Node): HTMLElement {
  return h("div", { class: "int-head" }, dot(COLORS[worst()], 7), h("b", { text: "System" }), h("span", { text: t("This computer") }), extra);
}

let armed: { pid: number; until: number } | null = null;

function processRow(p: Process, first: boolean): HTMLElement {
  const row = h("div", { class: first ? "int-row first" : "int-row" },
    h("span", { class: "int-name", text: p.name }),
    h("span", { class: "sys-num", text: `${p.cpu.toFixed(p.cpu < 10 ? 1 : 0)}%` }),
    h("span", { class: "sys-num", text: bytes(p.memory) }),
  );
  if (p.own) {
    const sure = armed?.pid === p.pid && armed.until > Date.now();
    const end = h("button", {
      class: sure ? "sys-end sure" : "sys-end",
      text: sure ? t("Sure?") : t("End"),
      onclick: (e: Event) => {
        e.stopPropagation();
        if (armed?.pid === p.pid && armed.until > Date.now()) {
          armed = null;
          void Bridge.systemEnd(p.pid);
        } else {
          armed = { pid: p.pid, until: Date.now() + ARMED_MS };
        }
        State.notify();
      },
    });
    row.append(end);
  }
  return row;
}

function processes(onBack: (() => void) | null): HTMLElement {
  const list = (Array.isArray(data().top) ? data().top : []) as Process[];
  const rows = h("div", { class: "int-rows" });
  list.forEach((p, i) => rows.append(processRow(p, i === 0)));
  if (!list.length) rows.append(h("div", { class: "sys-empty", text: "…" }));
  const head = h("div", { class: "int-detail-head" });
  if (onBack) head.append(h("button", { class: "int-back", onclick: onBack }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 })));
  head.append(h("b", { text: t("Processes") }), h("span", { class: "sys-cols", text: `${t("CPU")} · ${t("Memory")}` }));
  return h("div", { class: "int-card detail sys-procs" }, head, rows);
}

/** The card, the processes beside it with room, or behind it without. */
export function renderSystem(wide: boolean, detailOpen: boolean, open: () => void, close: () => void): HTMLElement {
  if (wide) return h("div", { class: "int-split" }, systemCard(null), processes(null));
  return detailOpen ? processes(close) : systemCard(open);
}
