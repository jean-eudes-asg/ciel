// Tools → Calendar's pure parts (src/core/calendar.ts): the grid, the days
// with events, and the world clocks' hours apart.

import { test } from "node:test";
import assert from "node:assert/strict";
import { busyDays, dayKey, eventsOn, firstDayOfWeek, hoursApart, monthGrid, validZone, zoneOffset } from "../src/core/calendar.ts";

test("the week starts on Sunday in the US, on Monday in France", () => {
  assert.equal(firstDayOfWeek("en-US"), 0);
  assert.equal(firstDayOfWeek("fr-FR"), 1);
  assert.equal(firstDayOfWeek("tr-TR"), 1);
});

test("six weeks from the week's first day, the month inside", () => {
  const sunday = monthGrid(2026, 10, 0);
  assert.equal(sunday.length, 42);
  // November 2026 starts on a Sunday: the grid starts with it.
  assert.equal(dayKey(sunday[0]), "2026-11-01");
  assert.equal(dayKey(sunday[41]), "2026-12-12");
  assert.equal(dayKey(monthGrid(2026, 10, 1)[0]), "2026-10-26");
  assert.equal(dayKey(monthGrid(2026, 11, 1)[0]), "2026-11-30");
});

test("events touch the days they cover; an all-day one ends before its midnight", () => {
  const at = (d, h = 0) => new Date(2026, 10, d, h).getTime();
  const events = [
    { title: "Stand-up", start: at(9, 9), end: at(9, 10), allDay: false },
    { title: "Trip", start: at(10), end: at(12), allDay: true },
    { title: "Late", start: at(9, 23), end: at(10, 1), allDay: false },
  ];
  assert.deepEqual([...busyDays(events)].sort(), ["2026-11-09", "2026-11-10", "2026-11-11"]);
  assert.deepEqual(eventsOn(events, new Date(2026, 10, 10)).map((e) => e.title), ["Trip", "Late"]);
  assert.deepEqual(eventsOn(events, new Date(2026, 10, 12)), []);
});

test("a world clock's hours apart, half hours too", () => {
  const at = Date.UTC(2026, 10, 9, 12);
  assert.equal(zoneOffset("Europe/Istanbul", at), 180);
  assert.equal(hoursApart("Europe/Istanbul", at, 120), "+1");
  assert.equal(hoursApart("Africa/Porto-Novo", at, 120), "-1");
  assert.equal(hoursApart("Asia/Kolkata", at, 120), "+3:30");
  assert.equal(hoursApart("Europe/Athens", at, 120), "");
  assert.ok(validZone("Africa/Porto-Novo"));
  assert.ok(!validZone("Mars/Olympus"));
});
