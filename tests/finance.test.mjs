// The Finance pill: the page's rules in src/core/finance.ts (numbers, the
// pill's text, the card's view model, the form's checks), the polling in
// src/island/finance.ts and the card in src/views/finance.ts. The files
// themselves — CSV, settings.json, the write — are tested in
// src-tauri/src/finance.rs.

import { after, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { calls, internals, sent } from "./tauri.mjs";
import { installFakeDom } from "./fakedom.mjs";
import {
  FINANCE_ID, budgetLevel, checkExpense, financeView, formatAmount, formatMoney, formatShort, isDate,
  isFullPath, localDate, parseAmount, pillText, readSummary, shortDate,
} from "../src/core/finance.ts";
import { applyFinance, financeOnScreen, registerFinanceHandlers } from "../src/island/finance.ts";
import { buildFinanceCard } from "../src/views/finance.ts";
import { DEFAULT_SETTINGS, State } from "../src/core/state.ts";
import { setLanguage } from "../src/i18n/i18n.ts";

installFakeDom();

/** A month of made-up spending, in a made-up base currency. */
const summary = (over = {}) => ({
  baseCurrency: "EUR",
  month: "2026-10",
  today: "2026-10-03",
  spentMonth: 1234.5,
  spentToday: 12,
  budget: 2000,
  remaining: 765.5,
  percentUsed: 61.725,
  topCategories: [
    { category: "Food", amount: 600 },
    { category: "Rent", amount: 500 },
    { category: "Fun", amount: 134.5 },
  ],
  recent: [
    { id: "0000000005", date: "2026-10-03", amount: 12, currency: "EUR", category: "Food", note: "lunch", createdAt: "2026-10-03T12:00:00" },
    { id: "0000000004", date: "2026-10-02", amount: 30, currency: "USD", category: "Fun", note: "", createdAt: "2026-10-02T20:00:00" },
  ],
  entryCount: 9,
  monthCount: 3,
  skippedRows: 0,
  unconverted: [],
  currencies: ["EUR", "GBP", "USD"],
  categories: ["Food", "Rent", "Fun"],
  monthlyIncome: 0,
  savingsBalance: 0,
  savingsCurrency: "EUR",
  ratesUpdated: null,
  ...over,
});

const poll = (over = {}) => ({ stamp: "s1", folder: "/data", summary: summary(), error: null, ...over });

// ── Numbers and the pill ──────────────────────────────────────────────────────

test("amounts have two decimals and the language's separators, in Latin digits", () => {
  assert.equal(formatAmount(1234.5, "en"), "1,234.50");
  assert.equal(formatAmount(0, "en"), "0.00");
  assert.equal(formatMoney(12, "TRY", "en"), "12.00 TRY");
  assert.equal(formatShort(2000, "en"), "2,000");
  assert.equal(formatShort(2000.5, "en"), "2,000.50");
  assert.match(formatAmount(1234.5, "fr"), /^1\s234,50$/);
  for (const lang of ["ar", "bn", "hi"]) {
    assert.ok(/^[0-9.,\s٫٬]+$/.test(formatAmount(1234.5, lang)), `${lang}: ${formatAmount(1234.5, lang)}`);
  }
});

test("the pill says how much of the budget is gone, else the month's total", () => {
  assert.equal(pillText(summary(), "en"), "62% of budget");
  assert.equal(pillText(summary({ budget: null, remaining: null, percentUsed: null }), "en"), "1,234.50 EUR this month");
  assert.equal(pillText(summary({ budget: null, percentUsed: null, spentMonth: 1200 }), "en"), "1,200 EUR this month");
  assert.equal(pillText(null), null);
  setLanguage("fr");
  try {
    assert.equal(pillText(summary()), "62 % du budget");
  } finally {
    setLanguage("en");
  }
});

// ── The card's view model ─────────────────────────────────────────────────────

test("the view model reads the month against the budget", () => {
  const v = financeView(summary(), "en");
  assert.equal(v.spent, "1,234.50");
  assert.equal(v.of, "/ 2,000 EUR");
  assert.equal(v.percent, "62%");
  assert.ok(Math.abs(v.fill - 0.61725) < 1e-9);
  assert.equal(v.level, "ok");
  assert.equal(v.remaining, "765.50 left");
  assert.equal(v.today, "12.00 today");
  assert.deepEqual(v.top.map((r) => [r.label, r.amount]), [["Food", "600.00"], ["Rent", "500.00"], ["Fun", "134.50"]]);
  assert.ok(Math.abs(v.top[0].share - 600 / 1234.5) < 1e-9);
  assert.deepEqual(v.recent[0], { id: "0000000005", date: "Oct 3", category: "Food", amount: "12.00 EUR", note: "lunch" });
  assert.equal(v.recent[1].amount, "30.00 USD");
  assert.equal(v.count, "3 entries this month");
  assert.equal(financeView(summary({ monthCount: 1 }), "en").count, "1 entry this month");
  assert.equal(v.warning, null);
});

test("over budget the bar is full and red, and the remaining says by how much", () => {
  const v = financeView(summary({ spentMonth: 2100, remaining: -100, percentUsed: 105 }), "en");
  assert.equal(v.fill, 1);
  assert.equal(v.level, "over");
  assert.equal(v.color, "#F4505E");
  assert.equal(v.remaining, "100.00 over");
  assert.equal(budgetLevel(79.9), "ok");
  assert.equal(budgetLevel(80), "near");
  assert.equal(budgetLevel(100), "near");
  assert.equal(budgetLevel(null), "ok");
});

test("without a budget there is no bar and no percentage", () => {
  const v = financeView(summary({ budget: null, remaining: null, percentUsed: null }), "en");
  assert.equal(v.of, "EUR");
  assert.equal(v.percent, null);
  assert.equal(v.fill, null);
  assert.equal(v.remaining, "No budget set");
});

test("what the totals leave out is said once", () => {
  const v = financeView(summary({ unconverted: ["JPY", "XAU"], skippedRows: 2 }), "en");
  assert.equal(v.warning, "No rate for JPY, XAU: not counted · 2 rows skipped");
  assert.equal(financeView(summary({ skippedRows: 1 }), "en").warning, "1 row skipped");
});

test("only something shaped like a summary is read as one", () => {
  assert.equal(readSummary(null), null);
  assert.equal(readSummary({}), null);
  assert.equal(readSummary({ baseCurrency: "EUR", spentMonth: "12" }), null);
  const s = readSummary({ baseCurrency: "EUR", spentMonth: 3, budget: 0, recent: [{ amount: "x" }, { amount: 2 }] });
  assert.equal(s.budget, null, "a budget of 0 is no budget");
  assert.equal(s.recent.length, 1);
  assert.deepEqual(s.currencies, []);
  assert.equal(shortDate("2026-01-09"), "Jan 9");
  assert.equal(shortDate("someday"), "someday");
});

// ── The form ──────────────────────────────────────────────────────────────────

test("amounts are typed with a dot or a decimal comma, two decimals at most", () => {
  for (const [text, value] of [["12", 12], ["12.5", 12.5], ["12,50", 12.5], [" 0.01 ", 0.01], ["1,234.56", 1234.56], ["1 234,5", 1234.5]]) {
    assert.equal(parseAmount(text), value, text);
  }
  for (const text of ["", "0", "0.00", "-3", "1.234", "abc", "12.", "1e3", "12,345", "Infinity"]) {
    assert.equal(parseAmount(text), null, text);
  }
});

test("dates are real days written YYYY-MM-DD, and today is local", () => {
  for (const ok of ["2026-01-31", "2024-02-29", "2000-02-29"]) assert.ok(isDate(ok), ok);
  for (const bad of ["2026-02-29", "1900-02-29", "2026-13-01", "2026-04-31", "26-10-01", "2026/10/01", ""]) {
    assert.ok(!isDate(bad), bad);
  }
  assert.equal(localDate(new Date(2026, 0, 5, 23, 59)), "2026-01-05");
});

test("the form is checked with the same rules and words as finance.rs", () => {
  const form = { amount: "12,50", currency: "usd", category: " Food ", note: "two\nlines", date: "2026-10-03" };
  const currencies = ["EUR", "USD"];
  assert.deepEqual(checkExpense(form, currencies), {
    ok: true,
    expense: { amount: 12.5, currency: "USD", category: "Food", note: "two lines", date: "2026-10-03" },
  });
  const error = (over) => checkExpense({ ...form, ...over }, currencies).error;
  assert.equal(error({ amount: "1.234" }), "Enter an amount above 0 with at most 2 decimals.");
  assert.equal(error({ date: "2026-02-30" }), "Enter the date as YYYY-MM-DD.");
  assert.equal(error({ currency: "JPY" }), "No exchange rate for JPY in settings.json.");
  assert.equal(error({ category: "" }), "Pick a category (60 characters at most).");
  assert.equal(error({ category: "x".repeat(61) }), "Pick a category (60 characters at most).");
  assert.equal(error({ note: "é".repeat(201) }), "Keep the note to 200 characters.");
  assert.equal(checkExpense({ ...form, category: "x".repeat(60), note: "é".repeat(200) }, currencies).ok, true);
});

test("a folder is a full path on its own system", () => {
  assert.ok(isFullPath("C:\\Users\\me\\data", "windows"));
  assert.ok(isFullPath("D:/dashboard/data", "windows"));
  assert.ok(isFullPath("\\\\nas\\share\\data", "windows"));
  assert.ok(!isFullPath("/home/me/data", "windows"));
  assert.ok(!isFullPath("data", "windows"));
  assert.ok(isFullPath("/home/me/data", "linux"));
  assert.ok(!isFullPath("C:\\data", "linux"));
  assert.ok(!isFullPath("~/data", "linux"));
  assert.equal(DEFAULT_SETTINGS.financeFolder, "", "empty means Ciel's own folder");
});

// ── Polling ───────────────────────────────────────────────────────────────────

const financeTask = () => ({
  id: FINANCE_ID, name: "Finance", color: "#10B981", state: "idle", stepIndex: 0, steps: [],
  source: "n8n", isIntegration: true,
});

beforeEach(() => {
  State.tasks = [financeTask()];
  State.focusId = FINANCE_ID;
  State.mode = "hidden";
  State.view = "overview";
  State.paused = false;
  delete State.integrations[FINANCE_ID];
  calls.length = 0;
});

after(() => {
  // No timer may outlive the tests.
  State.mode = "hidden";
  State.notify();
});

registerFinanceHandlers();

test("the files are only looked at while the pill is on screen", () => {
  State.notify();
  assert.equal(sent("finance_poll").length, 0, "hidden: nothing");
  State.mode = "compact";
  State.notify();
  assert.equal(sent("finance_poll").length, 0, "the compact island shows no text");
  State.mode = "expanded";
  State.notify();
  assert.ok(financeOnScreen());
  assert.equal(sent("finance_poll").length, 1, "open on the overview: at once");
  State.notify();
  assert.equal(sent("finance_poll").length, 1, "and then on its timer only");
  State.view = "prompt";
  State.notify();
  assert.ok(!financeOnScreen());
  State.view = "overview";
  State.paused = true;
  State.notify();
  assert.ok(!financeOnScreen(), "paused means paused");
  State.paused = false;
  State.tasks = [];
  State.notify();
  assert.ok(!financeOnScreen(), "not declared: nothing to look at");
  State.mode = "hidden";
  State.notify();
});

test("a reading fills the card's data and the pill's label; an error keeps what was shown", () => {
  applyFinance(poll());
  const info = State.integrations[FINANCE_ID];
  assert.equal(info.loaded, true);
  assert.equal(info.data.spentMonth, 1234.5);
  assert.equal(info.data.folder, "/data");
  assert.equal(State.tasks[0].name, "62% of budget");
  applyFinance(poll({ stamp: "s2", summary: null, error: "locked" }));
  assert.equal(State.integrations[FINANCE_ID].data.spentMonth, 1234.5);
  assert.equal(State.integrations[FINANCE_ID].error, "locked");
  // Back to its name when there is nothing to say.
  delete State.integrations[FINANCE_ID];
  State.notify();
  assert.equal(State.tasks[0].name, "Finance");
});

// ── The card ──────────────────────────────────────────────────────────────────

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const shown = (el) => el.style.display !== "none";

test("the card shows the month, the latest entries and adds an expense from its form", async () => {
  State.mode = "expanded";
  applyFinance(poll());
  const card = buildFinanceCard();
  const [summaryPane, entriesPane, addPane] = card.el.children;
  card.sync();
  assert.ok(shown(summaryPane) && !shown(entriesPane) && !shown(addPane));
  const text = summaryPane.textContent;
  for (const part of ["Finance", "Spending", "1,234.50", "/ 2,000 EUR", "62%", "765.50 left · 12.00 today", "Food", "600.00"]) {
    assert.ok(text.includes(part), part);
  }

  const [entriesBtn, addBtnIcon] = summaryPane.find(".fin-icon");
  entriesBtn.fire("click");
  assert.ok(shown(entriesPane) && !shown(summaryPane));
  assert.equal(entriesPane.find(".fin-entry").length, 2);
  assert.ok(entriesPane.textContent.includes("3 entries this month"));
  assert.ok(entriesPane.textContent.includes("lunch"));
  entriesPane.find(".gh-back")[0].fire("click");
  assert.ok(shown(summaryPane));

  addBtnIcon.fire("click");
  assert.ok(shown(addPane));
  assert.deepEqual(sent("focus_window").at(-1), { focused: true }, "the form gets the keyboard");
  const [amount] = addPane.find(".fin-amount");
  const [currency] = addPane.find(".fin-currency");
  const [category] = addPane.find(".fin-category");
  const [date] = addPane.find(".fin-date");
  assert.equal(currency.textContent, "EUR");
  assert.equal(category.textContent, "Food");
  assert.equal(date.value, localDate());

  // A currency is picked from the list the form gives way to.
  currency.fire("click");
  assert.ok(addPane.classList.contains("choosing"));
  const usd = addPane.find(".fin-chip").find((c) => c.textContent === "USD");
  usd.fire("click");
  assert.equal(currency.textContent, "USD");
  assert.ok(!addPane.classList.contains("choosing"));

  // Refused on the page: nothing reaches Rust.
  amount.value = "abc";
  addPane.find(".fin-add")[0].fire("click");
  await tick();
  assert.equal(sent("finance_add_expense").length, 0);
  assert.equal(addPane.find(".fin-error")[0].textContent, "Enter an amount above 0 with at most 2 decimals.");

  // Accepted: Rust adds it and answers with the new summary.
  const realInvoke = internals.invoke;
  internals.invoke = async (cmd, args) => {
    if (cmd === "finance_add_expense") {
      calls.push([cmd, args]);
      return poll({ stamp: "s3", summary: summary({ spentMonth: 1240.75, percentUsed: 62.04, remaining: 759.25 }) });
    }
    return realInvoke(cmd, args);
  };
  try {
    amount.value = "6,25";
    addPane.find(".fin-add")[0].fire("click");
    await tick();
    await tick();
  } finally {
    internals.invoke = realInvoke;
  }
  assert.deepEqual(sent("finance_add_expense"), [
    { expense: { amount: 6.25, currency: "USD", category: "Food", note: "", date: localDate() } },
  ]);
  card.sync();
  assert.ok(shown(summaryPane), "back on the summary");
  assert.ok(summaryPane.textContent.includes("1,240.75"));
  assert.deepEqual(sent("focus_window").at(-1), { focused: false }, "and the keyboard is given back");

  // Folding the island closes the form.
  addBtnIcon.fire("click");
  State.mode = "compact";
  card.sync();
  assert.ok(shown(summaryPane) && !shown(addPane));
  State.mode = "hidden";
  State.notify();
});

test("before the first reading the card says it is loading, and a failed one says why", () => {
  const card = buildFinanceCard();
  card.sync();
  assert.ok(card.el.textContent.includes("Loading…"));
  State.integrations[FINANCE_ID] = { data: {}, error: "Could not use the finance folder: denied", loaded: false, configured: true };
  card.sync();
  assert.ok(card.el.textContent.includes("Could not use the finance folder: denied"));
});
