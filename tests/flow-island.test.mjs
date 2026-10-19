// Dictation in the island: which of the pill's looks shows, when the island
// takes over from the pill, and how the dots move (src/core/flow.ts, shared by
// the pill and island/flow.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { dotGoal, islandFlowActive, pillLook, pillMessage } from "../src/core/flow.ts";
import { setLanguage } from "../src/i18n/i18n.ts";

setLanguage("en");

const state = (over = {}) => ({
  phase: "idle", edge: "island", key: "right_ctrl", noteStartedAt: null, announcement: null, enabled: true, voice: null,
  ...over,
});

test("the looks: a dictation first, then « OK Ciel », then a call, else at rest", () => {
  assert.equal(pillLook(state({ phase: "listening" })), "live");
  assert.equal(pillLook(state({ phase: "thinking" })), "live");
  assert.equal(pillLook(state({ phase: "locked" })), "handsFree");
  assert.equal(pillLook(state({ phase: "copied" })), "message");
  assert.equal(pillLook(state({ voice: { phase: "thinking", heard: "x", answer: "" }, noteStartedAt: 1 })), "voice");
  assert.equal(pillLook(state({ noteStartedAt: 1 })), "note");
  assert.equal(pillLook(state()), "resting");
  assert.equal(pillLook(state(), true), "toolbar");
  assert.equal(pillMessage("copied"), "Copied · press Ctrl+V to paste");
  assert.equal(pillMessage("idle"), null);
});

test("the island shows dictation only when it shows there and has something to show", () => {
  assert.equal(islandFlowActive(state()), false, "at rest, nothing: no resting pill in the island");
  assert.equal(islandFlowActive(state({ phase: "listening" })), true);
  assert.equal(islandFlowActive(state({ phase: "listening", edge: "bottom" })), false, "the pill has it");
  const announcement = { id: 1, icon: "download", tint: "#fff", text: "Downloading", action: null, actionId: null };
  assert.equal(islandFlowActive(state({ announcement })), true);
  assert.equal(islandFlowActive(state({ noteStartedAt: Date.now() })), true);
});

test("the dots: the voice while you speak, a wave while thinking, still at rest", () => {
  const voice = [0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
  assert.equal(dotGoal(state({ phase: "listening" }), voice, 0, 3, false), 0.3);
  assert.equal(dotGoal(state({ phase: "locked" }), voice, 0, 8, false), 0.8);
  const wave = dotGoal(state({ phase: "thinking" }), voice, 1000, 2, false);
  assert.ok(wave >= 0 && wave <= 0.5);
  assert.equal(dotGoal(state(), voice, 0, 0, false), 0);
  const speaking = state({ voice: { phase: "speaking", heard: "", answer: "Hi" } });
  assert.equal(dotGoal(speaking, voice, 0, 0, false), 0.05, "quiet until his voice is heard");
  assert.ok(dotGoal(speaking, voice, 500, 4, true) >= 0.2);
});
