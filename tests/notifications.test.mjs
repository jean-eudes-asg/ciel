// Notifications in the island: what Rust sends is checked and trimmed, and the
// queue keeps the newest first, one per tag (src/core/notifications.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { addNotification, MAX_QUEUED, parseNotification } from "../src/core/notifications.ts";

const raw = (over = {}) => ({
  id: "chrome-1", source: "chrome", app: "WhatsApp", title: "Ana", body: "Lunch?", icon: null, at: 1000, tag: null, ...over,
});

test("a notification from Rust is checked: its source, an id, something to show", () => {
  const n = parseNotification(raw());
  assert.equal(n.app, "WhatsApp");
  assert.equal(n.title, "Ana");
  assert.equal(parseNotification(raw({ source: "fax" })), null);
  assert.equal(parseNotification(raw({ id: "" })), null);
  assert.equal(parseNotification(raw({ title: " ", body: "" })), null);
  assert.equal(parseNotification(null), null);
  // A body alone becomes the title; an app missing gets its source's name.
  const bodyOnly = parseNotification(raw({ title: "", body: "Meeting in 5 min", app: "" }));
  assert.equal(bodyOnly.title, "Meeting in 5 min");
  assert.equal(bodyOnly.body, "");
  assert.equal(bodyOnly.app, "Chrome");
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
