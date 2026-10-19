// The Finance pill's card, in the overview's left card: this month against the
// budget, the top categories, the latest entries and an Add expense form, one
// pane at a time (the card is 108 px tall). What it says comes from
// core/finance.ts; the files are src-tauri/src/finance.rs's.
//
// Built once and updated in place, like Spotify's card: a poll that lands while
// an amount is being typed must not rebuild the form under the cursor. The
// form uses plain inputs and in-card choice lists — no native <select>, whose
// popup would open outside the island's shape.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import {
  FINANCE_GREEN, FINANCE_ID, MAX_NOTE, checkExpense, financeView, localDate, readSummary,
  type FinanceSummary,
} from "../core/finance";
import { pillDefinition } from "../core/pills";
import { addExpense } from "../island/finance";
import { language, t, tl } from "../i18n/i18n";

type Pane = "summary" | "entries" | "add";

export interface FinanceCardHost {
  el: HTMLElement;
  sync(): void;
  /** The card comes on screen again: it opens on the summary. */
  reset(): void;
}

function iconButton(icon: string, title: ReturnType<typeof tl>, onClick: () => void): HTMLElement {
  const b = h("button", { class: "int-more fin-icon", title }, svg(icon, 9));
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  return b;
}

function backButton(title: ReturnType<typeof tl>, onBack: () => void): HTMLElement {
  return h(
    "button",
    { class: "gh-back", onclick: onBack },
    svg(ICONS.chevronLeft, 9, { stroke: 2.6 }),
    h("span", { text: title }),
  );
}

export function buildFinanceCard(): FinanceCardHost {
  let pane: Pane = "summary";
  /** What the summary and entries panes were last drawn from. */
  let drawnKey = "";

  // ── Summary ────────────────────────────────────────────────────────────────
  const name = pillDefinition(FINANCE_ID)?.name ?? "Finance";
  const headDot = dot(FINANCE_GREEN, 7);
  const subtitle = h("span", { text: tl("Spending") });
  const head = h(
    "div",
    { class: "int-head fin-head" },
    headDot,
    h("b", { text: name }),
    subtitle,
    h("i", { class: "fin-grow" }),
    iconButton(ICONS.ellipsis, tl("Recent"), () => show("entries")),
    iconButton(ICONS.plus, tl("Add expense"), () => show("add")),
  );
  const summaryBody = h("div", { class: "fin-body" });
  const summaryPane = h("div", { class: "int-card fin-card" }, head, summaryBody);

  // ── Entries ────────────────────────────────────────────────────────────────
  const count = h("span", { class: "gh-head-right" });
  const entriesBody = h("div", { class: "fin-body" });
  const entriesPane = h(
    "div",
    { class: "int-card fin-card" },
    h("div", { class: "gh-detail-head" }, backButton(tl("Recent"), () => show("summary")), count),
    entriesBody,
  );

  // ── Add expense ────────────────────────────────────────────────────────────
  const form = { currency: "", category: "" };
  const amount = h("input", {
    class: "fin-input fin-amount", type: "text", inputmode: "decimal", placeholder: tl("Amount"),
    autocomplete: "off", spellcheck: "false",
  }) as HTMLInputElement;
  const currencyBtn = h("button", { class: "fin-input fin-pick fin-currency", title: tl("Currency") });
  const categoryBtn = h("button", { class: "fin-input fin-pick fin-category", title: tl("Category") });
  const date = h("input", {
    class: "fin-input fin-date", type: "text", placeholder: "YYYY-MM-DD", maxlength: "10",
    autocomplete: "off", spellcheck: "false",
  }) as HTMLInputElement;
  const note = h("input", {
    class: "fin-input fin-note", type: "text", placeholder: tl("Note"), maxlength: String(MAX_NOTE),
    autocomplete: "off", spellcheck: "false",
  }) as HTMLInputElement;
  const addBtn = h("button", { class: "fin-add", text: tl("Add") }) as HTMLButtonElement;
  const formError = h("span", { class: "fin-error" });
  const rows = h(
    "div",
    { class: "fin-form" },
    h("div", { class: "fin-line" }, amount, currencyBtn),
    h("div", { class: "fin-line" }, categoryBtn, date),
    h("div", { class: "fin-line" }, note, addBtn),
  );
  const choices = h("div", { class: "fin-choices" });
  const addPane = h(
    "div",
    { class: "int-card fin-card" },
    h("div", { class: "gh-detail-head" }, backButton(tl("Add expense"), () => show("summary")), formError),
    rows,
    choices,
  );

  const el = h("div", { class: "fin-root" }, summaryPane, entriesPane, addPane);

  /** Why the form was refused, in place of its title (two lines at most). */
  function setError(text: string) {
    formError.textContent = text;
    formError.title = text;
    addPane.classList.toggle("has-error", text !== "");
  }

  const summary = (): FinanceSummary | null => readSummary(State.integrations[FINANCE_ID]?.data);

  function show(next: Pane) {
    if (pane === next) return;
    const leavingForm = pane === "add";
    pane = next;
    closeChoices();
    if (next === "add") {
      resetForm();
      // The island never takes the keyboard on its own: lend it for the form.
      void Bridge.focusWindow(true);
      window.setTimeout(() => amount.focus(), 60);
    } else if (leavingForm) {
      void Bridge.focusWindow(false);
    }
    sync();
  }

  function resetForm() {
    const s = summary();
    amount.value = "";
    note.value = "";
    date.value = localDate();
    setError("");
    if (!form.currency || !(s?.currencies ?? []).includes(form.currency)) form.currency = s?.baseCurrency ?? "";
    if (!form.category || !(s?.categories ?? []).includes(form.category)) form.category = s?.categories[0] ?? "";
    drawPicks();
  }

  function drawPicks() {
    currencyBtn.textContent = form.currency || "—";
    categoryBtn.textContent = form.category || t("Category");
    categoryBtn.title = form.category;
  }

  // An in-card list in place of the form rows: pick one, the form comes back.
  let choosing: "currency" | "category" | null = null;
  function openChoices(kind: "currency" | "category") {
    const s = summary();
    const list = kind === "currency" ? (s?.currencies ?? []) : (s?.categories ?? []);
    if (list.length === 0) return;
    choosing = kind;
    clear(choices);
    const current = kind === "currency" ? form.currency : form.category;
    for (const value of list) {
      const chip = h("button", { class: value === current ? "fin-chip on" : "fin-chip", text: value, title: value });
      chip.addEventListener("click", (e) => {
        e.stopPropagation();
        if (kind === "currency") form.currency = value;
        else form.category = value;
        drawPicks();
        closeChoices();
      });
      choices.append(chip);
    }
    choices.classList.toggle("currencies", kind === "currency");
    addPane.classList.add("choosing");
  }
  function closeChoices() {
    choosing = null;
    addPane.classList.remove("choosing");
  }
  currencyBtn.addEventListener("click", () => (choosing === "currency" ? closeChoices() : openChoices("currency")));
  categoryBtn.addEventListener("click", () => (choosing === "category" ? closeChoices() : openChoices("category")));

  let sending = false;
  async function submit() {
    if (sending) return;
    const s = summary();
    const checked = checkExpense(
      { amount: amount.value, currency: form.currency, category: form.category, note: note.value, date: date.value },
      s?.currencies ?? [],
    );
    if (!checked.ok) return setError(checked.error);
    sending = true;
    addBtn.disabled = true;
    setError("");
    const error = await addExpense(checked.expense);
    sending = false;
    addBtn.disabled = false;
    if (error) return setError(error);
    show("summary");
  }
  addBtn.addEventListener("click", () => void submit());
  for (const input of [amount, date, note]) {
    // Clicking a field is asking to type in it.
    input.addEventListener("pointerdown", () => void Bridge.focusWindow(true));
    input.addEventListener("input", () => {
      if (formError.textContent) setError("");
    });
    input.addEventListener("keydown", (e) => {
      const key = (e as KeyboardEvent).key;
      if (key === "Enter") {
        e.preventDefault();
        void submit();
      } else if (key === "Escape") {
        e.preventDefault();
        if (choosing) closeChoices();
        else show("summary");
      }
      // Typing here is for the form, not for the island's keys.
      e.stopPropagation();
    });
  }
  // ── Drawing ────────────────────────────────────────────────────────────────

  function drawSummary(s: FinanceSummary | null, error: string | null) {
    clear(summaryBody);
    if (!s) {
      summaryBody.append(h("div", { class: "int-status" }, dot(error ? "#F4505E" : "#6B7079", 5),
        h("span", { text: error ?? t("Loading…") })));
      return;
    }
    const v = financeView(s);
    const total = h(
      "div",
      { class: "fin-total" },
      h("span", { class: "fin-spent", text: v.spent }),
      h("i", { text: v.of }),
    );
    if (v.percent) total.append(h("span", { class: "fin-pct", style: `color:${v.color}`, text: v.percent }));
    // What the totals leave out, or a reading that failed since: said on hover,
    // the card has no line to spare.
    const warning = error ?? v.warning;
    if (warning) {
      const mark = dot(error ? "#F4505E" : "#F5A524", 5);
      mark.title = warning;
      if (!v.percent) mark.style.marginLeft = "auto";
      total.append(mark);
    }
    summaryBody.append(total);
    if (v.fill != null) {
      const fill = h("i", { class: "fin-fill", style: `width:${(v.fill * 100).toFixed(1)}%;background:${v.color}` });
      summaryBody.append(h("div", { class: "fin-bar" }, fill));
    }
    summaryBody.append(h("div", { class: "fin-meta", text: `${v.remaining} · ${v.today}` }));
    const top = h("div", { class: "fin-top" });
    if (v.top.length === 0) top.append(h("div", { class: "fin-none", text: t("No expenses this month") }));
    for (const row of v.top) {
      top.append(
        h(
          "div",
          { class: "fin-cat" },
          h("i", { class: "fin-share", style: `width:${Math.max(4, row.share * 100).toFixed(0)}%` }),
          h("span", { class: "fin-cat-name", text: row.label, title: row.label }),
          h("span", { class: "fin-cat-amount", text: row.amount }),
        ),
      );
    }
    summaryBody.append(top);
  }

  function drawEntries(s: FinanceSummary | null) {
    clear(entriesBody);
    const v = s ? financeView(s) : null;
    count.textContent = v?.count ?? "";
    if (!v || v.recent.length === 0) {
      entriesBody.append(h("div", { class: "gh-empty", text: t("No expenses yet") }));
      return;
    }
    const list = h("div", { class: v.recent.length > 4 ? "fin-list fade" : "fin-list" });
    for (const e of v.recent) {
      const tip = [e.date, e.category, e.amount, e.note].filter(Boolean).join(" · ");
      list.append(
        h(
          "div",
          { class: "fin-entry", title: tip },
          h("span", { class: "fin-entry-date", text: e.date }),
          h("span", { class: "fin-entry-cat", text: e.category }),
          h("span", { class: "fin-entry-note", text: e.note }),
          h("span", { class: "fin-entry-amount", text: e.amount }),
        ),
      );
    }
    entriesBody.append(list);
  }

  function sync() {
    const info = State.integrations[FINANCE_ID];
    const s = summary();
    const task = State.tasks.find((x) => x.id === FINANCE_ID);
    const color = task?.color ?? FINANCE_GREEN;
    headDot.style.background = color;
    summaryPane.style.display = pane === "summary" ? "" : "none";
    entriesPane.style.display = pane === "entries" ? "" : "none";
    addPane.style.display = pane === "add" ? "" : "none";
    // Folded, or nothing read yet: the next look starts from the summary.
    if (pane !== "summary" && (State.mode !== "expanded" || !s)) return show("summary");

    const key = [language(), info?.error ?? "", JSON.stringify(info?.data ?? {})].join("~");
    if (key === drawnKey) return;
    drawnKey = key;
    drawSummary(s, info?.error ?? null);
    drawEntries(s);
    // A currency or category that left settings.json leaves the form too.
    if (s && !s.currencies.includes(form.currency)) form.currency = s.baseCurrency;
    if (s && !s.categories.includes(form.category)) form.category = s.categories[0] ?? "";
    drawPicks();
  }

  sync();
  return { el, sync, reset: () => show("summary") };
}
