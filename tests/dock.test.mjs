// The dock (src/core/dock.ts): what the island tells it of the agents and the
// music, what its agents' tile says, its clock, its icons growing under the
// pointer, where a dragged pin goes, and its sound icon.

import { test } from "node:test";
import assert from "node:assert/strict";

const {
  agentsLine, commandName, crewNow, dockLyric, dockSnapshot, dropIndex, playedAt, hour12, magnifyLayout, magnifyScale, untilNextMinute, volumeLevel, EMPTY_SNAPSHOT, MAGNIFY,
} = await import("../src/core/dock.ts");

const row = (phase, agent = "Claude Code", over = {}) => ({
  key: `${agent}:${phase}`, pillId: "p", sessionId: phase, agent, color: "#fff", project: "app", cwd: "/app",
  title: "", line: "", phase, since: 0, seen: 0, ...over,
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
  assert.equal(s.plan, null);
  const plan = { fiveHour: { usedPct: 42, resetsAt: 9e12 }, updatedAt: 0 };
  assert.equal(dockSnapshot([], null, plan).plan, plan);
});

const playing = (over = {}, lyrics = []) => ({ source: "spotify", state: music(over), lyrics });

test("what plays is sent, paused or not, never an ad", () => {
  assert.deepEqual(dockSnapshot([], playing()).playing, {
    title: "Song", artist: "Band", playing: true, source: "spotify", opens: true, position: 0, positionAt: 0, duration: 200, lyrics: [],
  });
  assert.equal(dockSnapshot([], playing({ playing: false })).playing.playing, false);
  assert.equal(dockSnapshot([], playing({ track: null })).playing, null);
  assert.equal(dockSnapshot([], playing({ track: { ...music().track, id: "spotify:ad:9" } })).playing, null);
  // Another app playing through the Spotify pill can't be brought forward from the dock.
  assert.equal(dockSnapshot([], playing({ spotify: false, player: "Firefox" })).playing.opens, false);
});

test("the dock sings the line of now, and knows when the next comes", () => {
  const words = [{ t: 10, text: "one" }, { t: 14, text: "two" }];
  const p = dockSnapshot([], playing({ position: 9, positionAt: 1000 }, words)).playing;
  const before = dockLyric(p, 1000);
  assert.equal(before.text, "");
  assert.equal(Math.round(before.nextIn), 850);
  assert.equal(dockLyric(p, 3000).text, "one");
  assert.deepEqual(dockLyric(p, 6000), { text: "two", nextIn: null });
  // Paused, it stays where it is and waits for nothing.
  const paused = { ...p, playing: false };
  assert.deepEqual(dockLyric(paused, 60_000), { text: "", nextIn: null });
  // Never past the end.
  assert.equal(playedAt(p, 10_000_000), 200);
});

test("the agents' tile says who waits for you first, then how many work", () => {
  assert.deepEqual(agentsLine(EMPTY_SNAPSHOT), { kind: "quiet" });
  assert.deepEqual(agentsLine(dockSnapshot([row("working"), row("working")], null)), { kind: "working", count: 2 });
  assert.deepEqual(agentsLine(dockSnapshot([row("working"), row("asking", "Codex")], null)), { kind: "asking", agent: "Codex" });
  // Done or failed sessions alone leave the tile quiet.
  assert.deepEqual(agentsLine(dockSnapshot([row("done"), row("error")], null)), { kind: "quiet" });
});

test("each session not idle has a dot under Ciel; a finished or failed one goes after a minute", () => {
  const s = dockSnapshot([row("asking", "Codex"), row("working"), row("done", "Gemini CLI", { since: 1000 }), row("idle")], null);
  assert.deepEqual(s.crew.map((m) => m.phase), ["asking", "working", "done"]);
  assert.equal(crewNow(s.crew, 1000 + 59_000).length, 3);
  assert.deepEqual(crewNow(s.crew, 1000 + 60_000).map((m) => m.phase), ["asking", "working"]);
  const failed = dockSnapshot([row("error", "Codex", { since: 0 })], null).crew;
  assert.equal(crewNow(failed, 61_000).length, 0);
});

test("a shell line comes down to the name of what it runs", () => {
  assert.equal(commandName("npm run build"), "npm");
  assert.equal(commandName("cd /home/me/app && cargo test --lib 2>&1 | tail"), "cargo");
  assert.equal(commandName("S=/tmp/x; python3 - <<E"), "python3");
  assert.equal(commandName("for i in $(seq 1 9); do pgrep -x ciel; done"), "pgrep");
  assert.equal(commandName("sudo timeout 60 /usr/bin/journalctl -u x"), "journalctl");
  assert.equal(commandName("LANG=C FOO=1 git status"), "git");
  assert.equal(commandName("S=/tmp/claude-1000/-home-jean-dev-ciel/72"), "");
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
