// The Printers pill's data as Rust sends it (src-tauri/src/printers/), and the
// words for it. Pure, so the island, Settings and the tests share it.

import { N_, t } from "../i18n/i18n";

export interface PrinterProblem {
  /** Stable: one of PROBLEM_LABELS' keys. */
  code: string;
  severity: "critical" | "warning" | "info";
  /** A cartridge and its level, or the printer's own words. */
  detail: string;
  sub: string;
}

export interface PrinterSupply {
  name: string;
  kind: string;
  color: string | null;
  /** What's left in percent, null when the printer doesn't say. */
  level: number | null;
}

export interface PrinterTray {
  name: string;
  level: number | null;
  empty: boolean;
}

export interface PrinterView {
  id: string;
  name: string;
  host: string;
  location: string;
  model: string;
  serial: string;
  panel: string;
  /** idle, printing, warmup, stopped, unknown, unreachable, or waiting before its first check. */
  state: string;
  via: string;
  problems: PrinterProblem[];
  supplies: PrinterSupply[];
  trays: PrinterTray[];
  pages: number | null;
  checkedAt: number;
  since: number;
  /** Problems in the last 30 days. */
  incidents: number;
  web: string;
  worst: PrinterProblem["severity"] | null;
}

/** printers/reading.rs PROBLEMS, the same English keys. */
export const PROBLEM_LABELS: Record<string, string> = {
  jammed: N_("Paper jam"),
  doorOpen: N_("Door open"),
  noPaper: N_("Out of paper"),
  noToner: N_("Toner empty"),
  offline: N_("Offline"),
  unreachable: N_("Not responding"),
  serviceRequested: N_("Service requested"),
  supplyMissing: N_("Cartridge missing"),
  outputFull: N_("Output tray full"),
  outputTrayMissing: N_("Output tray missing"),
  wasteFull: N_("Waste toner box full"),
  stopped: N_("Stopped"),
  lowPaper: N_("Paper low"),
  inputTrayEmpty: N_("Paper tray empty"),
  inputTrayMissing: N_("Paper tray missing"),
  lowToner: N_("Toner low"),
  outputNearFull: N_("Output tray almost full"),
  wasteNearFull: N_("Waste toner box almost full"),
  maintenance: N_("Maintenance due"),
  supplyLow: N_("Running low"),
  refused: N_("Credentials refused"),
  alert: N_("Printer alert"),
};

export const STATE_LABELS: Record<string, string> = {
  idle: N_("Idle"),
  printing: N_("Printing"),
  warmup: N_("Warming up"),
  stopped: N_("Stopped"),
  unknown: N_("Unknown"),
  unreachable: N_("Not responding"),
  waiting: N_("Not checked yet"),
};

export const SEVERITY_COLORS: Record<string, string> = { critical: "#F4505E", warning: "#F5A524", info: "#7FB4EA" };
export const OK_COLOR = "#22C55E";
const QUIET_COLOR = "#6B7079";

/** A problem in words, as report.rs `describe` says it. */
export function problemText(p: PrinterProblem): string {
  if ((p.code === "supplyLow" || p.code === "alert") && p.detail) return p.detail;
  const label = t(PROBLEM_LABELS[p.code] ?? N_("Printer alert"));
  return p.detail ? `${label} (${p.detail})` : label;
}

export function stateText(state: string): string {
  return t(STATE_LABELS[state] ?? N_("Unknown"));
}

/** A printer's dot: its worst problem, green when fine, grey before it's known. */
export function printerColor(p: PrinterView): string {
  if (p.worst) return SEVERITY_COLORS[p.worst] ?? SEVERITY_COLORS.warning;
  return p.state === "waiting" || p.state === "unknown" ? QUIET_COLOR : OK_COLOR;
}

/** A cartridge's bar colour: its own, but black shows light on the dark island. */
export function supplyColor(s: PrinterSupply): string {
  const c = s.color && /^#[0-9a-f]{6}$/i.test(s.color) ? s.color : null;
  if (!c) return "#9398A1";
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
  return r + g + b < 120 ? "#C5C8CD" : c;
}

/** The emptiest cartridge whose level is known, for a printer's short line. */
export function lowestSupply(p: PrinterView): PrinterSupply | null {
  const known = p.supplies.filter((s) => s.level != null && (s.kind === "toner" || s.kind === "ink"));
  return known.reduce<PrinterSupply | null>((low, s) => (low == null || (s.level ?? 0) < (low.level ?? 0) ? s : low), null);
}

/** What the card's row says under a printer's name. */
export function printerLine(p: PrinterView): string {
  if (p.problems.length > 0) return p.problems.map(problemText).join(" · ");
  const low = lowestSupply(p);
  const state = stateText(p.state);
  return low ? `${state} · ${low.name} ${low.level}%` : state;
}

const str = (v: unknown) => (typeof v === "string" ? v : "");
const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** The printers out of an integration update, each checked. */
export function readPrinters(data: Record<string, unknown> | undefined): PrinterView[] {
  const list = data?.printers;
  if (!Array.isArray(list)) return [];
  return list
    .filter((p): p is Record<string, unknown> => !!p && typeof p === "object" && typeof p.id === "string")
    .map((p) => ({
      id: str(p.id),
      name: str(p.name),
      host: str(p.host),
      location: str(p.location),
      model: str(p.model),
      serial: str(p.serial),
      panel: str(p.panel),
      state: str(p.state) || "unknown",
      via: str(p.via),
      problems: Array.isArray(p.problems) ? (p.problems as PrinterProblem[]).filter((q) => q && typeof q.code === "string") : [],
      supplies: Array.isArray(p.supplies) ? (p.supplies as PrinterSupply[]).filter((s) => s && typeof s.name === "string") : [],
      trays: Array.isArray(p.trays) ? (p.trays as PrinterTray[]).filter((s) => s && typeof s.name === "string") : [],
      pages: num(p.pages),
      checkedAt: num(p.checkedAt) ?? 0,
      since: num(p.since) ?? 0,
      incidents: num(p.incidents) ?? 0,
      web: /^https?:\/\//.test(str(p.web)) ? str(p.web) : "",
      worst: (["critical", "warning", "info"] as const).find((s) => s === p.worst) ?? null,
    }));
}

/** An id for a printer being added: its problems and history are filed under it. */
export function newPrinterId(now = Date.now(), random = Math.random()): string {
  return `p-${now.toString(36)}-${Math.floor(random * 36 ** 4).toString(36).padStart(4, "0")}`;
}

/** An address or a host name, as a printer can have: no spaces, no scheme, no path. */
export function isPrinterHost(s: string): boolean {
  const v = s.trim();
  return /^[A-Za-z0-9.:\-[\]]+$/.test(v) && v.length <= 253 && !v.startsWith("-");
}
