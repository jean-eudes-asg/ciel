// Ciel's brain as the Ciel window shows it (src/core/brain.ts): the setting's
// three values, what thinks now, and where what Ciel asks goes.

import { test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";

installFakeDom();

const { NO_BRAIN, aiEngine, brainPrivacy, brainState, engineChoices, localName, staysHere } = await import("../src/core/brain.ts");
const { DEFAULT_SETTINGS } = await import("../src/core/state.ts");
const { setLanguage } = await import("../src/i18n/i18n.ts");

const claude = { ...NO_BRAIN, engine: "claude" };
const ollama = { ...NO_BRAIN, engine: "local", server: "Ollama", model: "qwen3.5:9b", url: "http://127.0.0.1:11434", loopback: true };
const remote = { ...ollama, server: "OpenAI-compatible server", url: "https://llm.example.com", loopback: false };

test("the setting is Automatic by default and reads anything unknown as Automatic", () => {
  assert.equal(DEFAULT_SETTINGS.aiEngine, "auto");
  assert.equal(aiEngine("claude"), "claude");
  assert.equal(aiEngine("local"), "local");
  for (const odd of ["auto", "", "cloud", undefined, null, 3]) assert.equal(aiEngine(odd), "auto");
  assert.deepEqual(engineChoices().map(([id]) => id), ["auto", "claude", "local"]);
  assert.deepEqual(engineChoices().map(([, label]) => label), ["Automatic", "Claude", "Local model"]);
});

test("a local model is named with its server, and only one on this computer keeps everything here", () => {
  assert.equal(localName(ollama), "qwen3.5:9b (Ollama)");
  assert.equal(localName({ ...ollama, server: null }), "qwen3.5:9b");
  assert.equal(staysHere(ollama), true);
  assert.equal(staysHere(remote), false);
  assert.equal(staysHere(claude), false);
  assert.equal(staysHere(NO_BRAIN), false);
});

test("the state line says what thinks now, or what is missing for the choice", () => {
  assert.equal(brainState(claude), "Now: Claude, with your key.");
  assert.equal(brainState(ollama), "Now: qwen3.5:9b in Ollama.");
  assert.equal(brainState(NO_BRAIN), "Off: save a Claude key above, or connect a local model below.");
  assert.equal(brainState({ ...NO_BRAIN, pref: "claude" }), "Off: save a Claude key above.");
  assert.equal(brainState({ ...NO_BRAIN, pref: "local" }), "Off: connect a local model below.");
});

test("the privacy line says plainly when nothing leaves the computer", () => {
  assert.equal(brainPrivacy(ollama), "Nothing leaves this computer.");
  assert.equal(brainPrivacy(remote), "What Ciel asks the model is sent to https://llm.example.com.");
  assert.equal(brainPrivacy(claude), "What Ciel asks goes to Anthropic with your Claude key.");
  assert.equal(brainPrivacy(NO_BRAIN), null);
});

test("the lines are translated, placeholders filled", () => {
  setLanguage("fr");
  try {
    assert.equal(brainState(ollama), "En ce moment : qwen3.5:9b dans Ollama.");
    assert.equal(brainPrivacy(ollama), "Rien ne quitte cet ordinateur.");
    assert.equal(engineChoices()[2][1], "Modèle local");
  } finally {
    setLanguage("en");
  }
});
