// Notifications in the folded island, in a plain browser (`npx vite`,
// /dev/notes-preview.html): each case feeds the real card some notifications.

import "../src/style.css";
import { buildNotificationCard } from "../src/island/notifications";

/** A small coloured square as a site's icon. */
function icon(color: string): string {
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d")!;
  g.fillStyle = color;
  g.beginPath();
  g.arc(32, 32, 30, 0, Math.PI * 2);
  g.fill();
  return c.toDataURL("image/png");
}

const n = (id: string, over: Record<string, unknown>) => ({ id, source: "chrome", app: "WhatsApp", title: "", body: "", icon: null, at: Date.now(), tag: null, ...over });

const CASES: [string, Record<string, unknown>[]][] = [
  ["A message on WhatsApp Web", [n("a", { title: "Ana", body: "Are we still on for lunch at 1? I booked the place by the river.", icon: icon("#25D366") })]],
  ["New mail (Gmail), with two more waiting", [
    n("b1", { source: "gmail", app: "Gmail", title: "GitHub", body: "[ciel] CI passed on main — all 453 checks green" }),
    n("b2", { source: "gmail", app: "Gmail", title: "Stripe", body: "Your weekly summary" }),
    n("b3", { source: "gmail", app: "Gmail", title: "Léa Martin", body: "Re: the website — love the new sky!" }),
  ]],
  ["A site without an icon", [n("c", { app: "calendar.google.com", title: "Stand-up in 5 minutes", body: "Google Meet · 10:30 – 10:45" })]],
];

const root = document.getElementById("cases")!;
for (const [label, items] of CASES) {
  let deliver: (raw: unknown) => void = () => {};
  const card = buildNotificationCard(() => {}, () => {}, (fn) => (deliver = fn));
  const strip = document.createElement("div");
  strip.className = "strip";
  const ciel = document.createElement("i");
  ciel.className = "ciel";
  strip.append(ciel, card.el, card.pop);
  const box = document.createElement("div");
  box.className = "case";
  box.append(Object.assign(document.createElement("span"), { textContent: label }), strip);
  root.append(box);
  card.sync(true);
  // Oldest first: the newest is the one that shows. Again every few seconds,
  // before they fold away, so the preview always has them.
  let round = 0;
  const send = () => {
    round++;
    for (const item of [...items].reverse()) deliver({ ...item, id: `${item.id}-${round}` });
  };
  send();
  setInterval(send, 5000);
}
