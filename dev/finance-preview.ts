// The Finance pill's card and pill in a real island, on a made-up month (no
// one's real spending). `npm run dev`, then open /dev/finance-preview.html.
// The buttons below switch between a budget, no budget and over budget.
// Not shipped in the app.

import "../src/style.css";
import { Island } from "../src/island/island";
import { State } from "../src/core/state";
import { applyFinance } from "../src/island/finance";
import { FINANCE_ID, type FinanceSummary } from "../src/core/finance";

const island = new Island(document.getElementById("root")!);

const sample: FinanceSummary = {
  baseCurrency: "USD",
  month: "2026-10",
  today: "2026-10-19",
  spentMonth: 1234.5,
  spentToday: 18.4,
  budget: 2000,
  remaining: 765.5,
  percentUsed: 61.725,
  topCategories: [
    { category: "Groceries", amount: 412.3 },
    { category: "Eating out", amount: 268.9 },
    { category: "Transport", amount: 141 },
  ],
  recent: [
    { id: "a000000005", date: "2026-10-19", amount: 18.4, currency: "USD", category: "Eating out", note: "Lunch with the team", createdAt: "2026-10-19T12:40:00" },
    { id: "a000000004", date: "2026-10-18", amount: 54.2, currency: "EUR", category: "Groceries", note: "", createdAt: "2026-10-18T18:02:11" },
    { id: "a000000003", date: "2026-10-17", amount: 9.99, currency: "USD", category: "Subscriptions", note: "Music", createdAt: "2026-10-17T08:00:00" },
    { id: "a000000002", date: "2026-10-15", amount: 120, currency: "GBP", category: "Transport", note: "Train tickets, return", createdAt: "2026-10-15T21:30:45" },
    { id: "a000000001", date: "2026-10-12", amount: 35, currency: "USD", category: "Health", note: "Pharmacy", createdAt: "2026-10-12T10:10:10" },
  ],
  entryCount: 42,
  monthCount: 17,
  skippedRows: 0,
  unconverted: [],
  currencies: ["USD", "EUR", "GBP", "TRY"],
  categories: [
    "Housing", "Bills & Utilities", "Groceries", "Eating out", "Transport", "Phone & Internet",
    "Subscriptions", "Health", "Clothing", "Learning", "Entertainment", "Other",
  ],
};

const show = (summary: FinanceSummary) =>
  applyFinance({ stamp: String(Math.random()), folder: "/preview", summary, error: null });

State.settings.activeIntegrations = [FINANCE_ID, "integration_github", "integration_vercel"];
State.loadIntegrationTasks();
State.setFocus(FINANCE_ID);
show(sample);
island.alert("overview");
State.isPinned = true;
island.fsm.pinned = true;

const controls = document.getElementById("controls")!;
for (const [label, summary] of [
  ["Budget", sample],
  ["No budget", { ...sample, budget: null, remaining: null, percentUsed: null }],
  ["Over budget", { ...sample, spentMonth: 2104.75, remaining: -104.75, percentUsed: 105.2375, unconverted: ["JPY"] }],
] as const) {
  const b = document.createElement("button");
  b.textContent = label;
  b.onclick = () => show(summary as FinanceSummary);
  controls.append(b);
}
