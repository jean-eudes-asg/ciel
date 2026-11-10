// Notifications in the island: what Rust sends is checked and trimmed, and the
// queue keeps the newest first, one per tag (src/core/notifications.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { addNotification, cardButtons, isAnswerKeyCard, keepsCard, MAX_QUEUED, nextToShow, parseNotification } from "../src/core/notifications.ts";

const raw = (over = {}) => ({
  id: "chrome-1", source: "chrome", app: "WhatsApp", title: "Ana", body: "Lunch?", icon: null, at: 1000, tag: null, ...over,
});

test("a notification from Rust is checked: its source, an id, something to show", () => {
  const n = parseNotification(raw());
  assert.equal(n.app, "WhatsApp");
  assert.equal(n.title, "Ana");
  assert.equal(parseNotification(raw({ source: "fax" })), null);
  assert.equal(parseNotification(raw({ source: "desktop", app: "Thunderbird" })).source, "desktop");
  assert.equal(parseNotification(raw({ id: "" })), null);
  assert.equal(parseNotification(raw({ title: " ", body: "" })), null);
  assert.equal(parseNotification(null), null);
  // A body alone becomes the title; an app missing gets its source's name.
  const bodyOnly = parseNotification(raw({ title: "", body: "Meeting in 5 min", app: "" }));
  assert.equal(bodyOnly.title, "Meeting in 5 min");
  assert.equal(bodyOnly.body, "");
  assert.equal(bodyOnly.app, "Chrome");
});

test("during a call the Notetaker records, only the answer key's cards show; the others wait", () => {
  const mail = parseNotification(raw({ id: "gmail-1", source: "gmail", app: "Gmail" }));
  const answer = parseNotification(raw({ id: "answer-1", source: "ciel", app: "Ciel", title: "Price for ten seats", tag: "meeting-answer" }));
  const lookalike = parseNotification(raw({ id: "chrome-2", tag: "meeting-answer" }));
  assert.ok(isAnswerKeyCard(answer));
  assert.ok(!isAnswerKeyCard(lookalike), "a website's tag can't pass for Ciel's");
  let q = addNotification([], answer);
  q = addNotification(q, mail);
  assert.equal(nextToShow(q, false).id, "gmail-1", "no call: the newest");
  assert.equal(nextToShow(q, true).id, "answer-1", "in a call: the answer, the mail waits");
  assert.equal(nextToShow([mail, lookalike], true), null);
  assert.equal(nextToShow([], false), null);
});

test("an answer of the answer key's comes whole; anyone else's long text is cut", () => {
  const long = "The Pro plan is 90 euros per seat each month. ".repeat(20).trim();
  const answer = parseNotification(raw({ source: "ciel", app: "Ciel", title: "Price", body: long, tag: "meeting-answer" }));
  assert.equal(answer.body, long, `${long.length} characters, none lost`);
  const site = parseNotification(raw({ body: long, tag: "meeting-answer" }));
  assert.ok(site.body.length <= 600 && site.body.endsWith("…"));
});

test("the answer key's cards: Copy leaves the answer where it is, a word has Dismiss alone", () => {
  const ciel = { source: "ciel", app: "Ciel", tag: "meeting-answer" };
  const copy = [{ id: "copy", label: "Copy" }];
  const answer = parseNotification(raw({ ...ciel, title: "Price for ten seats", buttons: copy }));
  const thinking = parseNotification(raw({ ...ciel, title: "Ciel is thinking…", body: "" }));
  const mail = parseNotification(raw({ source: "gmail", app: "Gmail" }));
  assert.equal(thinking.body, "", "a title is enough");
  assert.deepEqual(cardButtons(answer, "Open"), copy);
  assert.deepEqual(cardButtons(thinking, "Open"), [], "nothing to open");
  assert.deepEqual(cardButtons(mail, "Open"), [{ id: "open", label: "Open" }]);
  assert.ok(keepsCard(answer, "copy"));
  assert.ok(!keepsCard(answer, "dismiss"));
  assert.ok(!keepsCard(parseNotification(raw({ buttons: copy })), "copy"), "a website's Copy is a button like any other");
});

test("only image data URLs are icons, and long text is cut", () => {
  assert.equal(parseNotification(raw({ icon: "https://example.com/i.png" })).icon, null);
  assert.equal(parseNotification(raw({ icon: "data:image/png;base64,AAAA" })).icon, "data:image/png;base64,AAAA");
  const long = parseNotification(raw({ body: "word ".repeat(400) }));
  assert.ok(long.body.length <= 600);
  assert.ok(long.body.endsWith("…"));
});

test("the queue: newest first, a tag replaces its older one, capped", () => {
  const a = parseNotification(raw({ id: "1", tag: "chat" }));
  const b = parseNotification(raw({ id: "2", tag: null }));
  const c = parseNotification(raw({ id: "3", tag: "chat" }));
  let q = addNotification([], a);
  q = addNotification(q, b);
  q = addNotification(q, c);
  assert.deepEqual(q.map((n) => n.id), ["3", "2"], "the chat's newer message replaced its older one");
  // The same tag from another app is another notification.
  const other = parseNotification(raw({ id: "4", tag: "chat", app: "Slack" }));
  assert.deepEqual(addNotification(q, other).map((n) => n.id), ["4", "3", "2"]);
  let many = [];
  for (let i = 0; i < MAX_QUEUED + 5; i++) many = addNotification(many, parseNotification(raw({ id: `n${i}` })));
  assert.equal(many.length, MAX_QUEUED);
  assert.equal(many[0].id, `n${MAX_QUEUED + 4}`);
});

test("the list grows to hold them all, up to three quarters of the display", async () => {
  const { inboxHeight, panelHeightFor, PANEL_H, VIEW_LAYOUTS } = await import("../src/core/layout.ts");
  const base = VIEW_LAYOUTS.notifications.height;
  assert.equal(inboxHeight(200, 1080), base, "never under its usual height");
  assert.equal(inboxHeight(520.4, 1080), 520, "as tall as its rows need");
  assert.equal(inboxHeight(5000, 1080), 810, "three quarters of the display, then it scrolls");
  assert.equal(inboxHeight(5000, 200), base, "a tiny display: its usual height still");
  // The window: the panel's, or the island plus a margin.
  assert.equal(panelHeightFor(160), PANEL_H);
  assert.equal(panelHeightFor(520), 528);
});

test("a meeting's card brings its own buttons, its time, and stays until the meeting began", async () => {
  const { showFor, timeSpan } = await import("../src/core/notifications.ts");
  const meeting = parseNotification(raw({
    id: "calendar-1", source: "ciel", app: "Calendar", title: "Q4 budget", body: "Ana, Paul +2",
    buttons: [{ id: "notes", label: "Take notes" }, { id: "join", label: "Join" }, { id: "dismiss", label: "x" }, { label: "no id" }],
    until: 90_000, time: { start: 60_000, end: 30_000 },
  }));
  // Dismiss is the island's own, and a button needs an id.
  assert.deepEqual(meeting.buttons, [{ id: "notes", label: "Take notes" }, { id: "join", label: "Join" }]);
  assert.equal(meeting.until, 90_000);
  // An end before the start is no end.
  assert.deepEqual(meeting.time, { start: 60_000, end: 60_000 });
  assert.equal(showFor(meeting, 7, 30_000), 60_000, "until its end, whatever the setting says");
  assert.equal(showFor(meeting, 7, 89_900), 1000, "a second at least");

  const plain = parseNotification(raw());
  assert.deepEqual([plain.buttons, plain.until, plain.time], [[], null, null]);
  assert.equal(showFor(plain, 10), 10_000);
  assert.equal(showFor(plain, 0), 0, "until opened or dismissed");
  assert.equal(showFor(plain, "x"), 7000);
  assert.equal(parseNotification(raw({ time: { start: "soon" } })).time, null);

  const span = timeSpan({ start: Date.UTC(2026, 9, 20, 13), end: Date.UTC(2026, 9, 20, 13, 30) }, "en-GB");
  assert.match(span, /^\d\d:\d\d – \d\d:\d\d$/);
});

test("an alert from a monitoring tool comes as a notification of its own source", () => {
  const n = parseNotification(raw({ source: "alerts", app: "Grafana", title: "Disk almost full", tag: "grafana:a1b2" }));
  assert.equal(n.source, "alerts");
  assert.equal(n.app, "Grafana");
  assert.equal(parseNotification(raw({ source: "alerts", app: "" })).app, "Alerts");
});

test("an update keeps its lines and reaches only the notification it names", async () => {
  const { applyUpdate, parseNotificationUpdate } = await import("../src/core/notifications.ts");
  const u = parseNotificationUpdate({ id: "answer-1", title: " Price ", body: "It's 90 euros a seat.\n\n  Discounts   start at twenty." });
  assert.deepEqual(u, { id: "answer-1", title: "Price", body: "It's 90 euros a seat.\nDiscounts start at twenty.", buttons: null });
  assert.equal(parseNotificationUpdate({ id: "", title: "x" }), null);
  assert.equal(parseNotificationUpdate({ id: "a", title: "", body: "" }), null);
  const copy = [{ id: "copy", label: "Copy" }];
  const ciel = { source: "ciel", app: "Ciel", tag: "meeting-answer" };
  const list = [
    parseNotification(raw({ ...ciel, id: "answer-1", title: "Ciel is thinking…", body: "", until: 5000 })),
    parseNotification(raw({ id: "chrome-2" })),
  ];
  const next = applyUpdate(list, u);
  assert.equal(next[0].title, "Price");
  assert.equal(next[0].until, 5000, "what it says, the rest as it was");
  assert.deepEqual(next[0].buttons, []);
  assert.equal(next[1].title, "Ana");
  assert.deepEqual(applyUpdate(list, { ...u, id: "gone" }), list);

  // Whole: its Copy, and its three minutes from now.
  const whole = parseNotificationUpdate({ id: "answer-1", title: "Price", body: "It's 90 euros a seat.", buttons: copy, until: 9000 });
  assert.deepEqual(whole.buttons, copy);
  assert.equal(whole.until, 9000);
  const done = applyUpdate(next, whole)[0];
  assert.deepEqual(done.buttons, copy);
  assert.equal(done.until, 9000);
  // A word in the answer's place folds as any notification does: no end of its own (0 from Rust).
  const word = parseNotificationUpdate({ id: "answer-1", title: "Answer", body: "Ciel hasn't heard the others.", until: 0 });
  assert.equal(word.until, null);
  assert.equal(applyUpdate(next, word)[0].until, null);
  // An answer is read whole, its lines kept; another card's text is cut as ever.
  const long = `${"word ".repeat(250)}\nend`;
  assert.ok(parseNotification(raw({ ...ciel, body: "One.\nTwo." })).body === "One.\nTwo.");
  assert.ok(applyUpdate(next, { ...u, body: long })[0].body.length > 1000);
  assert.ok(applyUpdate(list, { ...u, id: "chrome-2", body: long })[1].body.length <= 600);
});

test("the answer key's card changes in place: no new arrival, the answer whole", async () => {
  const { installFakeDom } = await import("./fakedom.mjs");
  installFakeDom();
  const { buildNotificationCard } = await import("../src/island/notifications.ts");
  let deliver = () => {};
  let update = () => {};
  const arrived = [];
  const card = buildNotificationCard(() => {}, (n) => arrived.push(n.id), (d) => (deliver = d), (u) => (update = u));
  card.sync(true);
  deliver(raw({ id: "answer-1", source: "ciel", app: "Ciel", title: "Ciel is thinking…", body: "", tag: "meeting-answer", until: Date.now() + 1000 }));
  assert.ok(card.pop.classList.contains("answer"));
  assert.doesNotMatch(card.pop.textContent, /Copy|Open/, "nothing to copy or open while Ciel thinks");
  update({ id: "answer-1", title: "Price for ten seats", body: "It's 90 euros a seat.\nDiscounts start at twenty." });
  assert.deepEqual(arrived, ["answer-1"], "an update is not a new notification");
  assert.match(card.pop.textContent, /Price for ten seats/);
  assert.match(card.pop.textContent, /Discounts start at twenty/);
  assert.doesNotMatch(card.pop.textContent, /Copy/, "not before the answer is whole");
  // Whole: Copy comes, on the same card.
  update({
    id: "answer-1", title: "Price for ten seats", body: "It's 90 euros a seat.\nDiscounts start at twenty.",
    buttons: [{ id: "copy", label: "Copy" }], until: Date.now() + 180_000,
  });
  assert.match(card.pop.textContent, /Copy/);
  assert.deepEqual(arrived, ["answer-1"]);
  assert.ok(card.active);
  // One that isn't showing changes nothing on the card.
  update({ id: "chrome-9", title: "Other", body: "x" });
  assert.doesNotMatch(card.pop.textContent, /Other/);
  card.sync(false);
});
