// An answer's audio reaches the pill as base64 and is played from a blob: URL
// (src/pill/audio.ts): the app's policy lets media come from blob: only, and
// a data: URL was refused, so every Kitten, ElevenLabs or voice server answer
// fell back to the system voice.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolveObjectURL } from "node:buffer";
import { audioUrl } from "../src/pill/audio.ts";

test("the audio becomes a blob: URL holding the same bytes and type", async () => {
  const bytes = Uint8Array.from([0x49, 0x44, 0x33, 0x04, 0x00, 0xff, 0x80]);
  const url = audioUrl(Buffer.from(bytes).toString("base64"), "audio/mpeg");
  assert.match(url, /^blob:/);
  const blob = resolveObjectURL(url);
  assert.equal(blob.type, "audio/mpeg");
  assert.deepEqual(new Uint8Array(await blob.arrayBuffer()), bytes);
  URL.revokeObjectURL(url);
});

test("the app's policy plays media from blob:, and the pill never uses data: for it", () => {
  const conf = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  for (const csp of [conf.app.security.csp, conf.app.security.devCsp]) {
    const media = csp.split(";").map((d) => d.trim()).find((d) => d.startsWith("media-src"));
    assert.ok(media?.split(/\s+/).includes("blob:"), media);
  }
  const pill = readFileSync(new URL("../src/pill/main.ts", import.meta.url), "utf8");
  assert.doesNotMatch(pill, /new Audio\(`data:/);
});
