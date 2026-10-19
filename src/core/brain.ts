// Ciel's brain as the Ciel window shows it (src-tauri/src/ai.rs): who thinks
// for « OK Ciel », the dictation clean-up and the call summaries — Claude with
// the user's key, or the local model picked for the chat. Pure helpers, so
// they can be tested without a webview.

import { t } from "../i18n/i18n";

/** Settings.aiEngine. */
export type AiEngine = "auto" | "claude" | "local";

/** What Rust reports in flow_status: the setting, and what thinks now. */
export interface AiStatus {
  pref: AiEngine;
  /** "off" when neither Claude nor a local model can. */
  engine: "claude" | "local" | "off";
  /** For a local model: its server ("Ollama"), the model, the address. */
  server: string | null;
  model: string | null;
  url: string | null;
  /** The local server runs on this computer. */
  loopback: boolean;
}

export const NO_BRAIN: AiStatus = { pref: "auto", engine: "off", server: null, model: null, url: null, loopback: false };

/** The setting as one of its three values: anything else reads as Automatic, as in Rust. */
export function aiEngine(value: unknown): AiEngine {
  return value === "claude" || value === "local" ? value : "auto";
}

/** The choices of the setting, for a select. */
export function engineChoices(): [AiEngine, string][] {
  return [["auto", t("Automatic")], ["claude", "Claude"], ["local", t("Local model")]];
}

/** A local model as people see it: "qwen3.5:9b (Ollama)". */
export function localName(ai: AiStatus): string {
  const model = ai.model ?? "";
  return ai.server ? `${model} (${ai.server})` : model;
}

/** Nothing Ciel asks its brain leaves this computer. */
export function staysHere(ai: AiStatus): boolean {
  return ai.engine === "local" && ai.loopback;
}

/** What thinks now, or what is missing, in one line. */
export function brainState(ai: AiStatus): string {
  if (ai.engine === "claude") return t("Now: Claude, with your key.");
  if (ai.engine === "local") return t("Now: {model} in {server}.", { model: ai.model ?? "", server: ai.server ?? "" });
  if (ai.pref === "claude") return t("Off: save a Claude key above.");
  if (ai.pref === "local") return t("Off: connect a local model below.");
  return t("Off: save a Claude key above, or connect a local model below.");
}

/** Where what Ciel asks goes; null when nothing thinks. */
export function brainPrivacy(ai: AiStatus): string | null {
  if (staysHere(ai)) return t("Nothing leaves this computer.");
  if (ai.engine === "local") return t("What Ciel asks the model is sent to {url}.", { url: ai.url ?? "" });
  if (ai.engine === "claude") return t("What Ciel asks goes to Anthropic with your Claude key.");
  return null;
}
