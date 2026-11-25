// The dock (src/core/dock.ts): what the island tells it of the agents and the
// music, what its agents' tile says, its clock, its icons growing under the
// pointer, where a dragged pin goes, and its sound icon.

import { test } from "node:test";
import assert from "node:assert/strict";

const {
  agentsLine, dockSnapshot, dropIndex, hour12, magnifyLayout, magnifyScale, untilNextMinute, volumeLevel, EMPTY_SNAPSHOT, MAGNIFY,
} = await import("../src/core/dock.ts");

const row = (phase, agent = "Claude Code") => ({
  key: `${agent}:${phase}`, pillId: "p", sessionId: phase, agent, color: "#fff", project: "app", cwd: "/app",
  title: "", line: "", phase, since: 0, seen: 0,
});

const music = (over = {}) => ({
  running: true, installed: true, playing: true, position: 0, positionAt: 0, shuffle: false, repeat: false,
  volume: 50, player: "Spotify", spotify: true, canSeek: true, canShuffle: true,
  track: { id: "spotify:track:1", title: "Song", artist: "Band", album: "", duration: 200, artUrl: null },
  ...over,
});

test("the sessions are counted, idle ones left out, and the one asking named", () => {
  const s = dockSnapshot([row("asking", "Codex"), row("working"), row("working", "Gemini CLI"), row("done"), row("idle")], null);
  assert.deepEqual(s.agents, { asking: 1, working: 2, done: 1, error: 0 });
  assert.equal(s.asker, "Codex");
  assert.equal(s.playing, null);
});

test("what plays is sent, paused or not, never an ad", () => {
  assert.deepEqual(dockSnapshot([], music()).playing, { title: "Song", artist: "Band", playing: true });
  assert.equal(dockSnapshot([], music({ playing: false })).playing.playing, false);
  assert.equal(dockSnapshot([], music({ track: null })).playing, null);
  const ad = music({ track: { ...music().track, id: "spotify:ad:9" } });
  assert.equal(dockSnapshot([], ad).playing, null);
});

test("the agents' tile says who waits for you first, then how many work", () => {
  assert.deepEqual(agentsLine(EMPTY_SNAPSHOT), { kind: "quiet" });
  assert.deepEqual(agentsLine(dockSnapshot([row("working"), row("working")], null)), { kind: "working", count: 2 });
  assert.deepEqual(agentsLine(dockSnapshot([row("working"), row("asking", "Codex")], null)), { kind: "asking", agent: "Codex" });
  // Done or failed sessions alone leave the tile quiet.
  assert.deepEqual(agentsLine(dockSnapshot([row("done"), row("error")], null)), { kind: "quiet" });
});

test("the clock follows Settings → Clock, and ticks just past each minute", () => {
  assert.equal(hour12("12h"), true);
  assert.equal(hour12("24h"), false);
  assert.equal(hour12("auto"), undefined);
  assert.equal(untilNextMinute(1_800_000_000_000), 60_050);
  assert.equal(untilNextMinute(1_800_000_059_000), 1_050);
});

test("an icon grows most under the pointer, less further off, not at all past its reach", () => {
  assert.equal(magnifyScale(0), MAGNIFY.max);
  const near = magnifyScale(20);
  const far = magnifyScale(60);
  assert.ok(near < MAGNIFY.max && far < near && far > 1);
  assert.equal(magnifyScale(-60), far, "the same either side");
  assert.equal(magnifyScale(MAGNIFY.reach), 1);
  assert.equal(magnifyScale(500), 1);
  assert.equal(magnifyScale(Number.NaN), 1);
});

const near = (a, b) => Math.abs(a - b) < 1e-9;

test("grown icons make room, the point under the pointer stays put", () => {
  // Three icons 40 wide with gaps of 2, the middle one doubled, the pointer on its middle.
  const items = [{ left: 0, width: 40, scale: 1 }, { left: 42, width: 40, scale: 2 }, { left: 84, width: 40, scale: 1 }];
  const { shifts, growLeft, growRight } = magnifyLayout(items, 62);
  assert.ok(near(shifts[1], 0), "the one under the pointer stays where it is");
  assert.ok(near(shifts[0], -20) && near(shifts[2], 20), "its neighbours each move half its growth");
  assert.ok(near(growLeft, 20) && near(growRight, 20));
  // Pointer on the left quarter of it: more of the growth goes right.
  const off = magnifyLayout(items, 52);
  assert.ok(near(off.shifts[0], -10) && near(off.shifts[2], 30));
  assert.ok(near(off.growLeft + off.growRight, 40));
  // At rest nothing moves.
  const rest = magnifyLayout(items.map((it) => ({ ...it, scale: 1 })), 62);
  assert.deepEqual(rest, { shifts: [0, 0, 0], growLeft: 0, growRight: 0 });
  assert.deepEqual(magnifyLayout([], 10), { shifts: [], growLeft: 0, growRight: 0 });
});

test("a dragged pin goes before the first pin right of the pointer, else last", () => {
  const centres = [20, 62, 104];
  assert.equal(dropIndex(centres, 5), 0);
  assert.equal(dropIndex(centres, 50), 1);
  assert.equal(dropIndex(centres, 90), 2);
  assert.equal(dropIndex(centres, 200), 3);
  assert.equal(dropIndex([], 10), 0);
});

test("the sound icon: off when muted or silent, then three steps", () => {
  assert.equal(volumeLevel(null, false), 0);
  assert.equal(volumeLevel(0.8, true), 0);
  assert.equal(volumeLevel(0, false), 0);
  assert.equal(volumeLevel(0.2, false), 1);
  assert.equal(volumeLevel(0.5, false), 2);
  assert.equal(volumeLevel(1, false), 3);
});
