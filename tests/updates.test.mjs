// Ciel's own updates: latest.json as the release workflow writes it
// (scripts/latest-json.mjs) and the line Settings shows (src/settings/updates.ts).
// Checking, installing and the signature are tested in src-tauri/src/updates.rs
// and Tauri's updater itself.

import { test } from "node:test";
import assert from "node:assert/strict";
import { latestJson } from "../scripts/latest-json.mjs";
import { updateLine } from "../src/settings/updates.ts";
import { setLanguage } from "../src/i18n/i18n.ts";

setLanguage("en");

test("latest.json: each signed package under the keys that pick it, nothing unsigned", () => {
  const json = latestJson({
    version: "0.2.7",
    repo: "jean-eudes-asg/ciel",
    notes: "What changed",
    date: "2026-10-21T10:00:00.000Z",
    sigs: {
      "Ciel-Windows-0.2.7-setup.exe.sig": "dW50cnVzdGVkIGNvbW1lbnQ=\n",
      "Ciel-Linux-0.2.7-x86_64.AppImage.sig": "bGludXg=",
    },
  });
  assert.equal(json.version, "0.2.7");
  assert.equal(json.pub_date, "2026-10-21T10:00:00.000Z");
  const exe = "https://github.com/jean-eudes-asg/ciel/releases/download/v0.2.7/Ciel-Windows-0.2.7-setup.exe";
  assert.deepEqual(json.platforms["windows-x86_64"], { signature: "dW50cnVzdGVkIGNvbW1lbnQ=", url: exe });
  assert.deepEqual(json.platforms["windows-x86_64-nsis"], json.platforms["windows-x86_64"]);
  assert.equal(json.platforms["linux-x86_64-appimage"].url.endsWith("/v0.2.7/Ciel-Linux-0.2.7-x86_64.AppImage"), true);
  assert.equal(json.platforms["windows-x86_64-msi"], undefined, "no signature, no entry: the updater would refuse it");
  assert.equal(json.platforms["linux-x86_64"], undefined, "a .deb or .rpm never gets the AppImage to install");
});

const status = (over = {}) => ({
  ready: true, current: "0.2.7", available: null, canInstall: true, busy: false, progress: null, error: null, checkedAt: 0, ...over,
});

test("Settings says where the updates stand", () => {
  assert.equal(updateLine(status()), "", "never checked: nothing to say");
  assert.equal(updateLine(status({ checkedAt: 1 })), "Ciel is up to date.");
  assert.equal(updateLine(status({ checkedAt: 1, available: "0.2.8" })), "Ciel 0.2.8 is out.");
  assert.equal(updateLine(status({ busy: true })), "Checking…");
  assert.equal(updateLine(status({ busy: true, progress: 0.426 })), "Downloading… 43%");
  assert.equal(updateLine(status({ error: "network down" })), "Something went wrong: network down");
  assert.match(updateLine(status({ ready: false })), /can't update itself/);
});
