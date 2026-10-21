// The Printers pill in the island (src-tauri/src/printers/): the card lists
// the printers, those with a problem first, each with its worst news; the
// detail has every printer's cartridges, trays and page count, Check now, and
// the way to its own web page.

import { h, svg, dot } from "./dom";
import { ICONS } from "./icons";
import { Bridge } from "../core/bridge";
import {
  OK_COLOR,
  SEVERITY_COLORS,
  printerColor,
  printerLine,
  problemText,
  readPrinters,
  stateText,
  supplyColor,
  type PrinterView,
} from "../core/printers";
import { t, tn } from "../i18n/i18n";

const COLOR = "#06B6D4";

function ago(at: number): string {
  if (!at) return "";
  const diff = (Date.now() - at) / 1000;
  if (diff < 60) return t("just now");
  if (diff < 3600) return t("{n}m", { n: Math.floor(diff / 60) });
  if (diff < 86400) return t("{n}h", { n: Math.floor(diff / 3600) });
  return t("{n}d", { n: Math.floor(diff / 86400) });
}

function head(extra?: Node): HTMLElement {
  const row = h("div", { class: "int-head" }, dot(COLOR, 7), h("b", { text: "Printers" }), h("span", { text: t("Building") }));
  if (extra) row.append(extra);
  return row;
}

/** The printers, the worst first; a press opens the detail. */
export function printersCard(data: Record<string, unknown>, rows: number, onDetail: () => void, onSettings: () => void): HTMLElement {
  const printers = readPrinters(data);
  const wrong = printers.filter((p) => p.problems.length > 0).length;
  const total = h(
    "span",
    { class: "int-total" },
    h("i", { class: "pulse", style: `background:${wrong > 0 ? SEVERITY_COLORS.critical : OK_COLOR}` }),
    h("span", { text: printers.length > 0 ? `${wrong}/${printers.length}` : "0" }),
  );
  const more = h("button", { class: "int-more", title: t("Details"), onclick: onDetail }, svg(ICONS.ellipsis, 8));
  const list = h("div", { class: "int-rows" });
  if (printers.length === 0) {
    const row = h("div", { class: "int-row first" }, dot(COLOR, 5), h("span", { class: "int-name", text: t("Add printers in Settings") }));
    row.style.cursor = "pointer";
    row.addEventListener("click", onSettings);
    list.append(row);
  }
  printers.slice(0, rows).forEach((p, i) => {
    const color = printerColor(p);
    const row = h(
      "div",
      { class: i === 0 ? "int-row first" : "int-row" },
      dot(color, 5),
      h("span", { class: "int-name", text: p.name }),
      h("span", { class: "int-sub", text: printerLine(p) }),
      h("span", { class: "int-ago", text: ago(p.checkedAt) }),
    );
    if (i === 0) row.style.background = `${color}14`;
    row.style.cursor = "pointer";
    row.addEventListener("click", onDetail);
    list.append(row);
  });
  if (printers.length > rows) {
    list.append(h("div", { class: "pr-more", text: t("+{n} more", { n: printers.length - rows }) }));
  }
  return h("div", { class: "int-card" }, head(h("span", { class: "pr-head-extra" }, total, more)), list);
}

function bar(name: string, level: number | null, color: string, warn = false): HTMLElement {
  const fill = h("i", { class: "pr-fill" });
  fill.style.width = `${level ?? 0}%`;
  fill.style.background = color;
  return h(
    "div",
    { class: "pr-bar" },
    h("span", { class: "pr-bar-name", text: name }),
    h("span", { class: "pr-track" }, fill),
    h("span", { class: warn ? "pr-bar-level low" : "pr-bar-level", text: level == null ? "–" : `${level}%` }),
  );
}

function printerBlock(p: PrinterView): HTMLElement {
  const color = printerColor(p);
  const place = [p.location, p.host].filter(Boolean).join(" · ");
  const block = h(
    "div",
    { class: "pr-printer" },
    h(
      "div",
      { class: "pr-title" },
      dot(color, 6),
      h("b", { text: p.name }),
      h("span", { class: "int-badge", style: `color:${color};background:${color}24`, text: stateText(p.state) }),
    ),
  );
  if (place) block.append(h("div", { class: "pr-place", text: place }));
  for (const problem of p.problems) {
    block.append(
      h("div", { class: "pr-problem" }, dot(SEVERITY_COLORS[problem.severity] ?? SEVERITY_COLORS.warning, 4), h("span", { text: problemText(problem) })),
    );
  }
  const lowNames = new Set(p.problems.filter((q) => q.code === "supplyLow").map((q) => q.sub));
  for (const s of p.supplies) {
    block.append(bar(s.name, s.level, supplyColor(s), lowNames.has(s.name)));
  }
  for (const tray of p.trays) {
    block.append(bar(tray.name, tray.level, "#6B7079"));
  }
  const facts: string[] = [];
  if (p.pages != null) facts.push(tn("{count} page", "{count} pages", p.pages));
  if (p.incidents > 0) facts.push(tn("{count} problem in 30 days", "{count} problems in 30 days", p.incidents));
  if (p.panel) facts.push(`« ${p.panel} »`);
  if (facts.length > 0) block.append(h("div", { class: "pr-facts", text: facts.join(" · ") }));
  if (p.web) {
    block.append(h("button", { class: "int-link", text: t("Open its web page"), onclick: () => void Bridge.openUrl(p.web) }));
  }
  return block;
}

/** Every printer in full, with Check now. */
export function printersDetail(data: Record<string, unknown>, onBack: () => void): HTMLElement {
  const printers = readPrinters(data);
  const check = h("button", { class: "int-link pr-check", text: t("Check now") });
  check.addEventListener("click", () => {
    check.textContent = t("Checking…");
    void Bridge.printersCheckNow();
  });
  const list = h("div", { class: "pr-list" }, ...printers.map(printerBlock));
  return h(
    "div",
    { class: "int-card detail" },
    h(
      "div",
      { class: "int-detail-head" },
      h("button", { class: "int-back", onclick: onBack }, svg(ICONS.chevronLeft, 10, { stroke: 2.4 })),
      dot(COLOR, 6),
      h("b", { text: "Printers" }),
      check,
    ),
    list,
  );
}
