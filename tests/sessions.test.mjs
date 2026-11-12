// The agents' list (src/core/sessions.ts): each session keeps its own line,
// whatever its pill shows, and the list says who needs you first.

import { test } from "node:test";
import assert from "node:assert/strict";

const { SessionStore, ageText, countSessions, phaseAfter } = await import("../src/core/sessions.ts");
const { agentsHeight, VIEW_LAYOUTS } = await import("../src/core/layout.ts");
const { isoWeek, nextEvent, weekDays } = await import("../src/core/calendar.ts");

const T0 = 1_800_000_000_000;
const CLAUDE = { pillId: "integration_claude", agent: "Claude Code", color: "#D97757" };
const CODEX = { pillId: "agent_codex", agent: "Codex", color: "#2DD4BF" };

/** One event of a session, `at` seconds after T0. */
const event = (store, who, sessionId, name, at = 0, extra = {}) =>
  store.observe({ event: name, sessionId, project: "app", cwd: "/home/me/app", now: T0 + at * 1000, ...who, ...extra });

test("two sessions of one agent keep a row each", () => {
  const store = new SessionStore();
  event(store, CLAUDE, "a", "UserPromptSubmit", 0, { prompt: "Fix the   tax\nrounding" });
  event(store, CLAUDE, "b", "UserPromptSubmit", 1, { prompt: "Write the docs" });
  event(store, CLAUDE, "a", "PreToolUse", 2, { line: "Runs · npm test" });
  assert.deepEqual(store.list.map((s) => [s.sessionId, s.title, s.line, s.phase]), [
    ["a", "Fix the tax rounding", "Runs · npm test", "working"],
    ["b", "Write the docs", "", "working"],
  ]);
});

test("a session goes from asked to working to done, and a new prompt starts it again", () => {
  const store = new SessionStore();
  event(store, CODEX, "c", "SessionStart", 0);
  assert.equal(store.list[0].phase, "idle");
  event(store, CODEX, "c", "UserPromptSubmit", 5, { prompt: "Rate limits" });
  assert.deepEqual([store.list[0].phase, store.list[0].since], ["working", T0 + 5000]);
  event(store, CODEX, "c", "PostToolUse", 9, { line: "Edits · limits.rs" });
  // Still working: the clock does not start again at each step.
  assert.deepEqual([store.list[0].line, store.list[0].since], ["Edits · limits.rs", T0 + 5000]);
  event(store, CODEX, "c", "Stop", 20, { final: "Added a token bucket.", line: "Edits · limits.rs" });
  assert.deepEqual([store.list[0].phase, store.list[0].line, store.list[0].since], ["done", "Added a token bucket.", T0 + 20_000]);
  event(store, CODEX, "c", "UserPromptSubmit", 30, { prompt: "And the tests" });
  assert.deepEqual([store.list[0].phase, store.list[0].title, store.list[0].line], ["working", "And the tests", ""]);
  event(store, CODEX, "c", "StopFailure", 40);
  assert.equal(store.list[0].phase, "error");
  event(store, CODEX, "c", "Interrupt", 41);
  assert.equal(store.list[0].phase, "idle");
});

test("the end of a session takes its row away, and a stray event never makes one", () => {
  const store = new SessionStore();
  event(store, CLAUDE, "a", "Notification", 0);
  event(store, CLAUDE, "a", "SessionEnd", 0);
  assert.deepEqual(store.list, []);
  event(store, CLAUDE, "a", "PreToolUse", 1, { line: "Reads · a.ts" });
  event(store, CLAUDE, "a", "Notification", 2);
  assert.equal(store.list.length, 1);
  event(store, CLAUDE, "a", "SessionEnd", 3);
  assert.deepEqual(store.list, []);
});

test("the one whose card waits is asking, and comes first", () => {
  const store = new SessionStore();
  event(store, CLAUDE, "a", "UserPromptSubmit", 0, { prompt: "One" });
  event(store, CODEX, "c", "UserPromptSubmit", 1, { prompt: "Two" });
  event(store, CODEX, "c", "Stop", 2);
  event(store, CLAUDE, "b", "UserPromptSubmit", 3, { prompt: "Three" });
  event(store, CLAUDE, "a", "PermissionRequest", 4);

  const now = T0 + 5000;
  // No card: the request went to the terminal, or a rule answered it.
  assert.deepEqual(store.rows(null, now).map((r) => [r.sessionId, r.phase]), [["b", "working"], ["a", "working"], ["c", "done"]]);
  const rows = store.rows({ pillId: "integration_claude", sessionId: "a" }, now);
  assert.deepEqual(rows.map((r) => [r.sessionId, r.phase]), [["a", "asking"], ["b", "working"], ["c", "done"]]);
  assert.deepEqual(countSessions(rows), { asking: 1, working: 1, done: 1, error: 0 });
  // The same session id under another agent's pill is another session.
  assert.equal(store.rows({ pillId: "agent_codex", sessionId: "a" }, now)[0].phase, "working");
  // Nothing was written back: asking is read from the card, never kept.
  assert.equal(store.list.find((s) => s.sessionId === "a").phase, "working");
});

test("Clear takes the finished ones, silence takes the rest, and the list has a cap", () => {
  const store = new SessionStore();
  event(store, CLAUDE, "a", "UserPromptSubmit", 0, { prompt: "One" });
  event(store, CODEX, "c", "Stop", 1);
  event(store, CODEX, "d", "StopFailure", 2);
  store.clearFinished();
  assert.deepEqual(store.list.map((s) => s.sessionId), ["a"]);

  // Four hours without a word: its terminal was closed.
  assert.equal(store.rows(null, T0 + 4 * 3600_000 + 1).length, 0);
  event(store, CODEX, "e", "UserPromptSubmit", 4 * 3600 + 2, { prompt: "Late" });
  assert.deepEqual(store.list.map((s) => s.sessionId), ["e"]);

  for (let n = 0; n < 20; n++) event(store, CLAUDE, `s${n}`, "UserPromptSubmit", 5 * 3600 + n, { prompt: "x" });
  assert.equal(store.list.length, 12);
  assert.ok(store.list.some((s) => s.sessionId === "s19") && !store.list.some((s) => s.sessionId === "s0"));
});

test("the demo's sessions are swapped in and out whole", () => {
  const store = new SessionStore();
  event(store, CLAUDE, "real", "UserPromptSubmit", 0, { prompt: "Real" });
  const real = store.swap();
  assert.deepEqual(store.list, []);
  event(store, CLAUDE, "demo", "UserPromptSubmit", 1, { prompt: "Demo" });
  store.swap(real);
  assert.deepEqual(store.list.map((s) => s.sessionId), ["real"]);
});

test("a long prompt is one line, cut; ages are short", () => {
  const store = new SessionStore();
  event(store, CLAUDE, "a", "UserPromptSubmit", 0, { prompt: "x".repeat(400) });
  assert.equal(store.list[0].title.length, 160);
  assert.ok(store.list[0].title.endsWith("…"));
  assert.deepEqual([0, 4999, 5000, 59_999, 60_000, 3_599_999, 3_600_000, 9 * 3_600_000].map(ageText),
    ["", "", "5s", "59s", "1m", "59m", "1h", "9h"]);
  assert.equal(phaseAfter("Notification"), null);
  assert.equal(phaseAfter("PermissionRequest"), "working");
});

test("the list's island grows with its rows, up to five", () => {
  assert.equal(agentsHeight(0), VIEW_LAYOUTS.agents.height);
  assert.equal(agentsHeight(1), VIEW_LAYOUTS.agents.height);
  assert.ok(agentsHeight(3) > agentsHeight(2));
  assert.equal(agentsHeight(5), agentsHeight(9));
});

test("the week a day is in, its number, and what comes next", () => {
  const thursday = new Date(2026, 10, 12, 21, 0);
  assert.deepEqual(weekDays(thursday, 0).map((d) => d.getDate()), [8, 9, 10, 11, 12, 13, 14]);
  assert.deepEqual(weekDays(thursday, 1).map((d) => d.getDate()), [9, 10, 11, 12, 13, 14, 15]);
  // Across a month's end.
  assert.deepEqual(weekDays(new Date(2026, 11, 1), 1).map((d) => d.getDate()), [30, 1, 2, 3, 4, 5, 6]);
  assert.equal(isoWeek(thursday), 46);
  assert.equal(isoWeek(new Date(2026, 0, 1)), 1);
  assert.equal(isoWeek(new Date(2027, 0, 1)), 53);
  assert.equal(isoWeek(new Date(2024, 11, 30)), 1);

  const now = thursday.getTime();
  const hour = 3600_000;
  const events = [
    { title: "Over", start: now - 2 * hour, end: now - hour, allDay: false },
    { title: "All day", start: now - 5 * hour, end: now + 5 * hour, allDay: true },
    { title: "Later", start: now + 3 * hour, end: now + 4 * hour, allDay: false },
    { title: "Now", start: now - hour / 2, end: now + hour / 2, allDay: false },
    { title: "Next week", start: now + 7 * 24 * hour, end: now + 7 * 24 * hour + hour, allDay: false },
  ];
  assert.equal(nextEvent(events, now).title, "Now");
  assert.equal(nextEvent(events, now + hour).title, "Later");
  assert.equal(nextEvent(events, now + 5 * hour), null);
});
