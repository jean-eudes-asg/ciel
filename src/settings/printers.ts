// Settings → Integrations → Printers: the printers the Printers pill checks
// (src-tauri/src/printers/), finding and adding them, how often and how they
// are asked (SNMP v1/v2c/v3, the passwords in the keychain), when a cartridge
// is low, the daily summary, and the reports.

import { Bridge, type DiscoverAnswer, type FoundPrinter } from "../core/bridge";
import {
  isPrinterHost,
  newPrinterId,
  printerColor,
  printerLine,
  readPrinters,
  type PrinterView,
} from "../core/printers";
import type { PrinterEntry, PrintersPrefs } from "../core/state";
import { toggle } from "../hub/ui";
import { N_, t, tn } from "../i18n/i18n";
import { clear, h } from "../views/dom";
import { statusDot } from "./parts";

export interface PrintersContext {
  /** settings.printers itself: changed in place, then `save`d. */
  prefs: PrintersPrefs;
  save(): Promise<void>;
  /** Which of the printers' keychain entries hold something. */
  present: Record<string, boolean>;
  /** "Windows Credential Manager" or the Secret Service. */
  store: string;
}

const COMMUNITY_KEY = "printers-community";
const AUTH_KEY = "printers-auth-password";
const PRIV_KEY = "printers-priv-password";

const INTERVALS = [1, 5, 15, 30, 60];
const protocols = (): [string, string][] => [["auto", t("SNMP, then IPP")], ["snmp", "SNMP"], ["ipp", "IPP"]];
const auths = (): [string, string][] => [["none", t("None")], ["md5", "MD5"], ["sha", "SHA-1"], ["sha256", "SHA-256"]];
const source = (s: string) => (s === "system" ? t("this computer") : s === "mdns" ? "Bonjour" : s.toUpperCase());

/** A drop-down of `options`, their labels as shown. */
function select(options: [string, string][], value: string, onChange: (v: string) => void): HTMLSelectElement {
  const el = h("select", {}) as HTMLSelectElement;
  for (const [v, label] of options) el.append(h("option", { value: v, text: label }));
  el.value = value;
  el.addEventListener("change", () => onChange(el.value));
  return el;
}

function input(value: string, placeholder: string, style = "flex:1 1 140px;min-width:0"): HTMLInputElement {
  return h("input", { type: "text", value, placeholder, autocomplete: "off", spellcheck: "false", style }) as HTMLInputElement;
}

function percentInput(value: number, placeholder: string): HTMLInputElement {
  return h("input", {
    type: "number", min: "0", max: "100", value: value > 0 ? String(value) : "", placeholder, style: "width:64px",
  }) as HTMLInputElement;
}

const clamp = (v: string, fallback: number) => {
  const n = Math.round(Number(v));
  return v.trim() === "" || !Number.isFinite(n) ? fallback : Math.min(100, Math.max(0, n));
};

export function printersSection(ctx: PrintersContext): HTMLElement {
  const { prefs } = ctx;
  let views: PrinterView[] = [];
  const list = h("div", { class: "pr-set-list" });
  const found = h("div", { class: "pr-set-list" });
  const findStatus = h("div", { class: "hint" });
  const reportStatus = h("div", { class: "hint" });

  async function refresh() {
    views = readPrinters((await Bridge.printersState()) ?? undefined);
    drawList();
  }

  const commit = async () => {
    await ctx.save();
    window.setTimeout(() => void refresh(), 4000);
  };

  // ── The printers ──────────────────────────────────────────────────────────

  function drawList() {
    clear(list);
    if (prefs.list.length === 0) {
      list.append(h("div", { class: "hint", text: t("No printers yet: find them below, or add one by its address.") }));
      return;
    }
    for (const p of prefs.list) list.append(printerRow(p));
  }

  function printerRow(p: PrinterEntry): HTMLElement {
    const view = views.find((v) => v.id === p.id);
    const dotEl = statusDot(true);
    dotEl.style.background = view ? printerColor(view) : "#6b7079";
    const name = input(p.name, view?.model || t("Name"));
    const host = input(p.host, "192.168.1.40", "flex:0 1 150px;min-width:0");
    const place = input(p.location, view?.location || t("Location"));
    const warn = percentInput(p.tonerWarn, String(prefs.tonerWarn));
    const critical = percentInput(p.tonerCritical, String(prefs.tonerCritical));
    const protocol = select(protocols(), p.protocol || "auto", (v) => {
      p.protocol = v as PrinterEntry["protocol"];
      void commit();
    });
    name.addEventListener("change", () => {
      p.name = name.value.trim();
      void commit();
    });
    place.addEventListener("change", () => {
      p.location = place.value.trim();
      void commit();
    });
    host.addEventListener("change", () => {
      if (!isPrinterHost(host.value)) {
        host.value = p.host;
        return;
      }
      p.host = host.value.trim();
      void commit();
    });
    warn.addEventListener("change", () => {
      p.tonerWarn = clamp(warn.value, 0);
      void commit();
    });
    critical.addEventListener("change", () => {
      p.tonerCritical = clamp(critical.value, 0);
      void commit();
    });
    const remove = h("button", { text: t("Remove") });
    remove.addEventListener("click", () => {
      prefs.list = prefs.list.filter((x) => x.id !== p.id);
      drawList();
      drawFound();
      void commit();
    });
    const line = view ? printerLine(view) : t("Not checked yet");
    return h(
      "div",
      { class: "pr-set-printer" },
      h("div", { class: "row" }, dotEl, name, host, protocol, remove),
      h(
        "div",
        { class: "row" },
        place,
        h("span", { class: "hint", text: t("Low at") }), warn,
        h("span", { class: "hint", text: t("Nearly empty at") }), critical,
      ),
      h("div", { class: "hint", text: line }),
    );
  }

  function add(entry: Omit<PrinterEntry, "id" | "protocol" | "tonerWarn" | "tonerCritical">) {
    if (prefs.list.some((p) => p.host === entry.host)) return;
    prefs.list = [...prefs.list, { id: newPrinterId(), protocol: "auto", tonerWarn: 0, tonerCritical: 0, ...entry }];
    drawList();
    drawFound();
    void commit();
  }

  // ── Adding by address ─────────────────────────────────────────────────────

  const newHost = input("", "192.168.1.40", "flex:0 1 150px;min-width:0");
  const newName = input("", t("Name"));
  const addBtn = h("button", { text: t("Add") });
  addBtn.addEventListener("click", () => {
    if (!isPrinterHost(newHost.value)) {
      newHost.focus();
      return;
    }
    add({ host: newHost.value.trim(), name: newName.value.trim(), location: "" });
    newHost.value = "";
    newName.value = "";
  });

  // ── Finding ───────────────────────────────────────────────────────────────

  let last: DiscoverAnswer | null = null;

  function drawFound() {
    clear(found);
    for (const f of last?.found ?? []) found.append(foundRow(f));
  }

  function foundRow(f: FoundPrinter): HTMLElement {
    const added = prefs.list.some((p) => p.host === f.host);
    const button = h("button", { text: added ? t("Added") : t("Add") }) as HTMLButtonElement;
    button.disabled = added;
    button.addEventListener("click", () => add({ host: f.host, name: f.name, location: f.location }));
    const about = [f.host, f.model, f.location, f.sources.map(source).join(", ")]
      .filter(Boolean)
      .join(" · ");
    return h(
      "div",
      { class: "row" },
      h("span", { style: "flex:1 1 auto;min-width:0" }, h("b", { text: f.name || f.host }), h("div", { class: "hint", text: about })),
      button,
    );
  }

  const range = input(prefs.sweepRange, t("This computer's network (/24)"), "flex:1 1 220px;min-width:0");
  range.addEventListener("change", () => {
    prefs.sweepRange = range.value.trim();
    void ctx.save();
  });
  const findBtn = h("button", { text: t("Find printers") }) as HTMLButtonElement;
  const sweepBtn = h("button", { text: t("Sweep the network") }) as HTMLButtonElement;
  async function find(sweep: boolean) {
    findBtn.disabled = sweepBtn.disabled = true;
    findStatus.textContent = sweep ? t("Asking every address in the range…") : t("Looking for printers…");
    try {
      last = await Bridge.printersDiscover(sweep);
      const n = last.found.length;
      findStatus.textContent = sweep
        ? tn("{count} printer found, {swept} addresses asked in {range}.", "{count} printers found, {swept} addresses asked in {range}.", n, {
          swept: last.swept,
          range: last.range,
        })
        : tn("{count} printer found.", "{count} printers found.", n);
      drawFound();
    } catch (e) {
      findStatus.textContent = String(e);
    } finally {
      findBtn.disabled = sweepBtn.disabled = false;
    }
  }
  findBtn.addEventListener("click", () => void find(false));
  sweepBtn.addEventListener("click", () => void find(true));

  // ── Checking ──────────────────────────────────────────────────────────────

  const every = select(
    INTERVALS.map((n) => [String(n), t("Every {n} min", { n })]),
    String(INTERVALS.includes(prefs.intervalMinutes) ? prefs.intervalMinutes : 5),
    (v) => {
      prefs.intervalMinutes = Number(v);
      void ctx.save();
    },
  );
  const checkBtn = h("button", { text: t("Check now") });
  checkBtn.addEventListener("click", () => {
    void Bridge.printersCheckNow();
    window.setTimeout(() => void refresh(), 5000);
  });

  // ── SNMP ──────────────────────────────────────────────────────────────────

  /** A keychain entry's field. `optional`: empty is fine (the community is then "public"), so its dot isn't red. */
  function secretRow(key: string, label: string, placeholder: string, optional = false): HTMLElement {
    const emptyColor = optional ? "#6b7079" : "#f4505e";
    const field = h("input", {
      type: "password",
      placeholder: ctx.present[key] ? `••••••••  ${t("(stored)")}` : placeholder,
      autocomplete: "off",
      spellcheck: "false",
      style: "flex:1 1 160px;min-width:0",
    }) as HTMLInputElement;
    const dotEl = statusDot(ctx.present[key] ?? false);
    if (!ctx.present[key]) dotEl.style.background = emptyColor;
    const saveBtn = h("button", { text: t("Save") });
    saveBtn.addEventListener("click", async () => {
      const value = field.value.trim();
      try {
        await Bridge.secretSet(key, value);
        ctx.present[key] = value.length > 0;
        field.value = "";
        field.placeholder = value ? `••••••••  ${t("(stored)")}` : placeholder;
        dotEl.style.background = value ? "#22c55e" : emptyColor;
      } catch {
        dotEl.style.background = "#f5a524";
      }
    });
    return h("div", { class: "row" }, h("label", { text: t(label) }), field, saveBtn, dotEl);
  }

  const snmpRows = h("div", { style: "display:flex;flex-direction:column;gap:10px" });
  function drawSnmp() {
    clear(snmpRows);
    const version = select([["v1", "SNMP v1"], ["v2c", "SNMP v2c"], ["v3", "SNMP v3"]], prefs.snmpVersion, (v) => {
      prefs.snmpVersion = v as PrintersPrefs["snmpVersion"];
      void ctx.save();
      drawSnmp();
    });
    snmpRows.append(h("div", { class: "row" }, h("label", { text: t("Version") }), version));
    if (prefs.snmpVersion !== "v3") {
      snmpRows.append(secretRow(COMMUNITY_KEY, N_("Community"), "public", true));
      return;
    }
    const user = input(prefs.snmpUser, t("User name"));
    user.addEventListener("change", () => {
      prefs.snmpUser = user.value.trim();
      void ctx.save();
    });
    snmpRows.append(
      h("div", { class: "row" }, h("label", { text: t("User name") }), user),
      h("div", { class: "row" }, h("label", { text: t("Authentication") }), select(auths(), prefs.snmpAuth, (v) => {
        prefs.snmpAuth = v;
        void ctx.save();
        drawSnmp();
      })),
    );
    if (prefs.snmpAuth === "none") return;
    snmpRows.append(
      secretRow(AUTH_KEY, N_("Authentication password"), t("8 characters or more")),
      h("div", { class: "row" }, h("label", { text: t("Encryption (AES-128)") }), toggle(prefs.snmpPrivacy, (on) => {
        prefs.snmpPrivacy = on;
        void ctx.save();
        drawSnmp();
      })),
    );
    if (prefs.snmpPrivacy) snmpRows.append(secretRow(PRIV_KEY, N_("Encryption password"), t("8 characters or more")));
  }

  // ── When to tell ──────────────────────────────────────────────────────────

  const warnAll = percentInput(prefs.tonerWarn, "15");
  const criticalAll = percentInput(prefs.tonerCritical, "5");
  warnAll.addEventListener("change", () => {
    prefs.tonerWarn = clamp(warnAll.value, 15);
    void commit();
  });
  criticalAll.addEventListener("change", () => {
    prefs.tonerCritical = clamp(criticalAll.value, 5);
    void commit();
  });
  const summary = h("input", { type: "time", value: prefs.dailySummary, style: "width:110px" }) as HTMLInputElement;
  summary.addEventListener("change", () => {
    prefs.dailySummary = summary.value;
    void ctx.save();
  });

  // ── Reports ───────────────────────────────────────────────────────────────

  const copyBtn = h("button", { text: t("Copy report") });
  copyBtn.addEventListener("click", async () => {
    const ok = await Bridge.printersCopyReport();
    reportStatus.textContent = ok ? t("Copied") : "";
  });
  const exportBtn = h("button", { text: t("Export CSV") });
  exportBtn.addEventListener("click", async () => {
    try {
      const folder = await Bridge.printersExport();
      reportStatus.textContent = t("Saved in {folder}", { folder });
    } catch (e) {
      reportStatus.textContent = String(e);
    }
  });

  drawList();
  drawSnmp();
  void refresh();
  // Settings is open: what the printers say now, every half minute.
  const timer = window.setInterval(() => {
    if (!list.isConnected) {
      window.clearInterval(timer);
      return;
    }
    if (!document.hidden) void refresh();
  }, 30_000);

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: "Printers" })),
    h("div", { class: "hint", text: t("While the Printers pill is on, Ciel asks each printer below how it is, over SNMP or IPP, and tells you when one has a problem and when it's fixed.") }),
    list,
    h("div", { class: "row" }, h("label", { text: t("Add by address") }), newHost, newName, addBtn),
    h("div", { class: "row" }, findBtn, sweepBtn, range),
    findStatus,
    h("div", { class: "hint", text: t("Find printers looks at the ones this computer knows and the ones that announce themselves. A sweep asks every address of the range over SNMP: some security tools take that for an attack, so tell your network admin first.") }),
    found,
    h("div", { class: "row" }, h("label", { text: t("Check") }), every, checkBtn),
    snmpRows,
    h("div", { class: "hint", text: t("Most printers answer SNMP v2c with the community \"public\"; the ones that don't are read over IPP. Passwords are kept in the {store}.", { store: ctx.store }) }),
    h(
      "div",
      { class: "row" },
      h("label", { text: t("Toner low at") }), warnAll,
      h("span", { class: "hint", text: t("Nearly empty at") }), criticalAll,
    ),
    h("div", { class: "row" }, h("label", { text: t("Daily summary") }), summary),
    h("div", { class: "hint", text: t("A notification each day at this time with how every printer is. Leave it empty for none.") }),
    h("div", { class: "row" }, h("label", { text: t("Also in the Alerts pill") }), toggle(prefs.toAlerts, (on) => {
      prefs.toAlerts = on;
      void ctx.save();
    })),
    h("div", { class: "row" }, copyBtn, exportBtn),
    reportStatus,
    h("div", { class: "hint", text: t("The report has every printer now and the last 30 days of problems. Export CSV writes two files for a spreadsheet.") }),
  );
}
