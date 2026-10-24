// Long answers said a piece at a time (src/pill/chunks.ts): every word kept,
// in order; no piece too long; cut at sentence ends, else commas, else spaces;
// numbers like 3.5 left whole; short sentences said together.

import { test } from "node:test";
import assert from "node:assert/strict";
import { speechChunks } from "../src/pill/chunks.ts";

const words = (pieces) => pieces.join(" ").split(/\s+/);

test("a long answer is cut where a voice pauses, nothing lost", () => {
  const sentence = "The weather in Paris is mild today, with a light breeze from the west and clouds that should clear by the afternoon";
  const text = `${sentence}. ${sentence}, so you won't need an umbrella, though a jacket may help this evening when it cools to twelve degrees! Anything else?`;
  const pieces = speechChunks(text, 120);
  assert.deepEqual(words(pieces), text.split(/\s+/));
  assert.ok(pieces.every((p) => p.length <= 120), JSON.stringify(pieces));
  assert.ok(pieces.length >= 3);
  assert.ok(pieces.some((p) => p.endsWith(",")), "a long sentence is cut after a comma");
});

test("short sentences go together, and 3.5 stays one number", () => {
  assert.deepEqual(speechChunks("It's 16:45. Your next meeting is at 17:00. It takes 3.5 hours."), [
    "It's 16:45. Your next meeting is at 17:00. It takes 3.5 hours.",
  ]);
  assert.deepEqual(speechChunks("One. Two.", 5), ["One.", "Two."]);
  assert.deepEqual(speechChunks("  "), []);
});

test("a sentence without a comma is cut at a space, a word without one at the limit", () => {
  assert.deepEqual(speechChunks("aaaa bbbb cccc", 9), ["aaaa bbbb", "cccc"]);
  assert.deepEqual(speechChunks("abcdefghij", 4), ["abcd", "efgh", "ij"]);
});
