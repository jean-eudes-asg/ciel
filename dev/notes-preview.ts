// Notifications in the folded island, in a plain browser (`npx vite`,
// /dev/notes-preview.html): each case feeds the real card some notifications.

import "../src/style.css";
import { buildNotificationCard } from "../src/island/notifications";
import { State } from "../src/core/state";
import { buildInbox, buildNoteStack } from "../src/views/inbox";
import { inboxHeight } from "../src/core/layout";
import type { ViewActions } from "../src/views/views";

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

// The notifications Ciel keeps (src/views/inbox.ts): the apps waiting beside
// Ciel in the folded island, and their list in the open one.
const minutesAgo = (m: number) => Date.now() - m * 60_000;
State.notifications = [
  n("i1", { source: "desktop", app: "Thunderbird", title: "Léa Martin", body: "Re: the website. Love the new sky! Can we ship it before Friday?", icon: icon("#0a84ff"), at: minutesAgo(0) }),
  n("i2", { app: "WhatsApp", title: "Ana", body: "Are we still on for lunch at 1?", icon: icon("#25D366"), at: minutesAgo(4) }),
  n("i3", { source: "desktop", app: "Slack", title: "#design", body: "Marc: the new island list looks great", icon: icon("#e01e5a"), at: minutesAgo(38) }),
  n("i4", { source: "gmail", app: "Gmail", title: "GitHub", body: "[ciel] CI passed on main", at: minutesAgo(140) }),
  n("i5", { source: "desktop", app: "Software Updater", title: "Updated software is available", body: "", at: minutesAgo(60 * 26) }),
  n("i6", { app: "Discord", title: "#general", body: "Sam: anyone up for a game tonight?", icon: icon("#5865F2"), at: minutesAgo(60 * 27) }),
  n("i7", { app: "Google Calendar", title: "Stand-up in 5 minutes", body: "Google Meet · 10:30 – 10:45", at: minutesAgo(60 * 28) }),
  n("i8", { source: "gmail", app: "Gmail", title: "Stripe", body: "Your weekly summary is ready", at: minutesAgo(60 * 30) }),
] as typeof State.notifications;

const inboxRoot = document.getElementById("inbox")!;
const folded = document.createElement("div");
folded.className = "strip";
const foldedCiel = document.createElement("i");
foldedCiel.className = "ciel";
const stack = buildNoteStack();
folded.append(foldedCiel, stack.el);
stack.sync(true);

const actions = { blip() {}, setView() {} } as unknown as ViewActions;
const inbox = buildInbox(actions);
inbox.el.classList.add("on");
const open = document.createElement("div");
open.className = "open";
open.innerHTML = '<div class="head"><span style="font:600 11px system-ui;color:#8e939c">header</span></div><div class="body"></div><i class="ciel"></i>';
open.querySelector(".body")!.append(inbox.el);
inbox.sync();
// As the island does (island.ts fitInbox): the list holds them all, up to
// three quarters of the display, then scrolls.
const fit = () => requestAnimationFrame(() => {
  open.style.height = `${inboxHeight(open.offsetHeight + (inbox.shortfall?.() ?? 0), window.screen.availHeight)}px`;
});
State.subscribe(() => {
  inbox.sync();
  stack.sync(true);
  fit();
});
inboxRoot.append(folded, open);
fit();
