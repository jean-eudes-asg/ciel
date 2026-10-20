// What « OK Ciel » is told about the service pills (src/island/service-cards.ts): what the pills say, and never an
// address, a subject, a description, a link or an error's text.

import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { serviceCards } from "../src/island/service-cards.ts";
import { State } from "../src/core/state.ts";

function service(id, data, extra = {}) {
  State.integrations[id] = { data, error: null, loaded: true, configured: true, ...extra };
}

afterEach(() => {
  State.tasks = State.tasks.filter((x) => x.id !== "integration_n8n");
  for (const id of Object.keys(State.integrations)) delete State.integrations[id];
});

test("service cards say what the pills say and never an address, a subject, a description or a link", () => {
  const now = Date.now();
  service("integration_stripe", { balance: 1250, currency: "eur", payments: [{ status: "succeeded", amount: 900, description: "Alice Martin invoice 42", createdAt: now - 120_000 }, { status: "failed", amount: 100, createdAt: now - 3_600_000 * 3 }] });
  service("integration_resend", { total: 31, emails: [{ to: ["alice@example.com"], subject: "Your secret order", lastEvent: "delivered", createdAt: now - 60_000 }] });
  service("integration_notion", { pages: [{ title: "Roadmap", url: "https://notion.so/secret", lastEditedAt: now - 5_000 }] });
  service("integration_github", { totalStars: 1520, totalRepos: 12 });
  service("integration_vercel", { deployments: [{ projectName: "shop", state: "READY", createdAt: now - 600_000 }] });
  service("integration_calcom", { bookings: [{ title: "Intro call", start: new Date(now + 3_600_000).toISOString() }] });
  const cards = serviceCards(now);
  const byId = Object.fromEntries(cards.map((c) => [c.id, c]));
  assert.equal(byId.integration_stripe.headline, "12.50 EUR");
  assert.deepEqual(byId.integration_stripe.items[0], { label: "Payment", detail: "+9.00 · 2m" });
  assert.equal(byId.integration_stripe.items[1].label, "Payment failed");
  assert.equal(byId.integration_resend.headline, "31");
  assert.deepEqual(byId.integration_resend.items, [{ label: "Delivered", detail: "1m" }]);
  assert.equal(byId.integration_notion.items[0].label, "Roadmap");
  assert.equal(byId.integration_github.headline, "1.5k Total stars");
  assert.equal(byId.integration_vercel.items[0].label, "shop");
  assert.equal(byId.integration_calcom.items[0].label, "Intro call");
  const all = JSON.stringify(cards);
  for (const secret of ["alice", "secret", "Alice", "invoice", "https://", "example.com"]) assert.ok(!all.includes(secret), `${secret} must not be in a card`);
});

test("a service that is not configured or is failing has no card, and n8n shows its name and result only", () => {
  service("integration_stripe", { balance: 1 }, { configured: false });
  service("integration_github", { totalStars: 1 }, { error: "boom" });
  State.tasks = State.tasks.filter((x) => x.id !== "integration_n8n");
  assert.deepEqual(serviceCards().map((c) => c.id), []);
  const n8n = { id: "integration_n8n", name: "n8n", color: "#fff", state: "error", stepIndex: 0, steps: ["Nightly sync", "Error: password=hunter2 at node 3"], source: "n8n", isIntegration: true };
  State.tasks.push(n8n);
  const card = serviceCards().find((c) => c.id === "integration_n8n");
  assert.equal(card.headline, "Failed");
  assert.deepEqual(card.items, [{ label: "Nightly sync", detail: "" }]);
  assert.ok(!JSON.stringify(card).includes("hunter2"));
});

test("the Calendar's card says the next meetings' titles and times, never who comes or where", () => {
  const now = Date.UTC(2026, 9, 20, 9, 0);
  service("integration_calendar", {
    events: [
      { id: "a", title: "Done already", start: now - 7_200_000, end: now - 3_600_000, allDay: false },
      { id: "b", title: "Standup", start: now - 300_000, end: now + 600_000, allDay: false, join: "https://meet.google.com/abc-defg-hij" },
      { id: "c", title: "", start: now + 3_600_000, end: now + 5_400_000, allDay: false, where: "Room 4, Alice's office" },
    ],
  });
  const card = serviceCards(now).find((c) => c.id === "integration_calendar");
  assert.equal(card.title, "Calendar");
  assert.deepEqual(card.items.map((i) => i.label), ["Standup", "Untitled"], "the one under way, then the next");
  assert.match(card.headline, /^20\/10 \d\d:\d\d$/);
  const text = JSON.stringify(card);
  for (const secret of ["https://", "Room 4", "Alice"]) assert.ok(!text.includes(secret), `${secret} must not be in the card`);

  service("integration_calendar", { events: [] });
  assert.equal(serviceCards(now).find((c) => c.id === "integration_calendar").headline, "No meetings coming up");
});
