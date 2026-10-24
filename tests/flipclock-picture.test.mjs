// The flip clock drawn for the lock screen (src/views/flipclock-picture.ts) on
// a recording fake canvas: below Windows' own clock and on the screen, as big
// as the screensaver's when it fits; the hours and minutes in the format
// chosen; the date only when wanted; a PNG at the screen's size.

import { test } from "node:test";
import assert from "node:assert/strict";
import { FakeCanvas, installFakeCanvas } from "./fakecanvas.mjs";

installFakeCanvas();
const { clockLayout, clockPng, paintFlipClock, WINDOWS_CLOCK_BOTTOM } = await import("../src/views/flipclock-picture.ts");

const NINE_FORTY_ONE = new Date(2026, 9, 24, 9, 41);
const texts = (ctx) => ctx.calls.filter((c) => c[0] === "fillText").map((c) => c[1]);

test("the cards and the date stay under Windows' clock and on the screen", () => {
  for (const [W, H] of [[3440, 1440], [1920, 1080], [1366, 768], [1080, 1920]]) {
    for (const size of ["small", "medium", "large"]) {
      for (const date of [true, false]) {
        const l = clockLayout(W, H, { size, date });
        assert.ok(l.top >= WINDOWS_CLOCK_BOTTOM * H - 0.5, `${W}x${H} ${size}: below Windows' clock`);
        const bottom = l.dateTop == null ? l.top + l.cardH : l.dateTop + l.dateSize;
        assert.ok(bottom <= H, `${W}x${H} ${size}: on the screen`);
        assert.ok(l.left >= 0 && l.left + 2 * l.cardW + l.gap <= W + 0.5, `${W}x${H} ${size}: within the width`);
        assert.equal(l.dateTop == null, !date);
      }
    }
  }
});

test("the screensaver's size when it fits, smaller only when it doesn't", () => {
  // Small on 1080p: the screensaver's min(19vh, 11vw).
  assert.equal(clockLayout(1920, 1080, { size: "small", date: true }).size, Math.min(19 * 10.8, 11 * 19.2));
  // Large on an ultrawide wouldn't fit under Windows' clock: it shrinks, still bigger than medium.
  const large = clockLayout(3440, 1440, { size: "large", date: true });
  const medium = clockLayout(3440, 1440, { size: "medium", date: true });
  assert.ok(large.size < Math.min(46 * 14.4, 29 * 34.4));
  assert.ok(large.size > medium.size);
});

test("24 hours, 12 hours with AM / PM, and the date only when wanted", () => {
  const canvas = new FakeCanvas(1920, 1080);
  paintFlipClock(canvas.getContext(), 1920, 1080, NINE_FORTY_ONE, { format: "24h", size: "large", date: true, lang: "en-US" });
  assert.deepEqual(texts(canvas.getContext()), ["09", "41", "SATURDAY, OCTOBER 24"]);

  const twelve = new FakeCanvas(1920, 1080);
  paintFlipClock(twelve.getContext(), 1920, 1080, NINE_FORTY_ONE, { format: "12h", size: "large", date: false, lang: "en-US" });
  assert.deepEqual(texts(twelve.getContext()), ["9", "41", "AM"]);

  // Pitch black under it all.
  assert.deepEqual(twelve.getContext().calls.find((c) => c[0] === "fillRect"), ["fillRect", 0, 0, 1920, 1080]);
});

test("a PNG at the screen's size", async () => {
  const png = await clockPng(3440, 1440, NINE_FORTY_ONE, { format: "auto", size: "large", date: true, lang: "fr" });
  assert.equal(new TextDecoder().decode(png), "PNG 3440x1440");
});
