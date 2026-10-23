// « OK Ciel »: say it, then ask anything. Turning it on, how Ciel answers
// out loud, and what leaves the computer. Port of the Mac's Settings → Voice.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { Bridge } from "../../core/bridge";
import { FlowBridge, type AgentStatus, type VoiceOutput } from "../../core/flow";
import { localName, staysHere, type AiStatus } from "../../core/brain";
import type { HubContext } from "../shell";
import { button, card, divider, label, number, row, segmented, toggle } from "../ui";
import { KITTEN, modelsControl } from "./flowsettings";

const ELEVEN = "elevenlabs-api-key";
/** Where secrets.rs keeps the keys on this OS (same words as Notch → Chat). */
const KEY_STORE = navigator.userAgent.includes("Windows") ? "Windows Credential Manager" : "Secret Service (GNOME Keyring, KWallet)";

function elevenKey(): HTMLElement {
  const box = h("div", { style: "display:flex;align-items:center;gap:8px" });
  const draw = async () => {
    const present = (await Bridge.secretPresent(ELEVEN)) ?? false;
    clear(box);
    if (present) {
      box.append(
        h("span", { class: "hub-note", style: "color:var(--hub-green)", text: `✓ ${t("Key saved in the {store}.", { store: KEY_STORE })}` }),
        button(t("Remove"), () => void Bridge.secretClear(ELEVEN).then(draw), "", true),
      );
      return;
    }
    const input = h("input", { class: "hub-input", type: "password", placeholder: t("ElevenLabs key"), autocomplete: "off", spellcheck: "false", style: "width:220px" }) as HTMLInputElement;
    const save = button(t("Save key"), () => {
      const value = input.value.trim();
      if (!value) return;
      void Bridge.secretSet(ELEVEN, value).then(draw, () => input.classList.add("err"));
    }, "primary", true);
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") save.click();
    });
    box.append(input, save);
  };
  void draw();
  return box;
}

export async function voicePage(page: HTMLElement, ctx: HubContext) {
  const status = await ctx.refresh();
  const prefs = status.prefs;
  page.append(ctx.hero("voice", "OK Ciel", t("Say « OK Ciel », then ask anything: add a pill, play music, the weather, an email to prepare.")));

  const ready = status.models.parakeet.state === "ready";
  const agent = await FlowBridge.agentStatus();
  const agentReady = !!agent?.built && !!agent.path && agent.signedIn && prefs.voiceAgent === true;
  page.append(label(t("Listening")));
  const main = card(
    row(t("Listen for « OK Ciel »"), t("The microphone stays open; only what follows « OK Ciel » goes further."),
      toggle(prefs.voice, (on) => void ctx.setPrefs((p) => (p.voice = on)).then(() => ctx.navigate("voice")))),
    divider(),
    row(t("Parakeet, on your computer"), t("Hears « OK Ciel » here, without the internet."), modelsControl(ctx, () => ctx.navigate("voice"))),
  );
  if (prefs.voice && status.ai.engine === "off" && !agentReady) {
    main.append(h("div", {
      class: "hub-warn",
      text: status.ai.pref === "claude"
        ? t("Ciel answers with Claude: add your Claude key in Notch → Chat.")
        : t("Ciel needs a brain to answer: add your Claude key or connect a local model in Notch → Chat."),
    }));
  } else if (prefs.voice && !ready) {
    main.append(h("div", { class: "hub-warn", text: t("Download the voice model so Ciel can hear you.") }));
  }
  page.append(main);

  // Only in a build that has it (the `claude-code` feature): off by default.
  if (agent?.built) {
    page.append(label("Claude Code"));
    page.append(claudeCodeCard(agent, prefs.voiceAgent === true, (on) => void ctx.setPrefs((p) => (p.voiceAgent = on)).then(() => ctx.navigate("voice"))));
  }

  page.append(label(t("Answers")));
  const outputs: [VoiceOutput, string][] = [["system", t("System voice")], ["kitten", "Kitten"], ["elevenlabs", "ElevenLabs"], ["off", t("Text only")]];
  const answers = card(
    row(t("Ciel answers out loud"), null, segmented(outputs, prefs.voiceOutput, (v) => void ctx.setPrefs((p) => (p.voiceOutput = v)).then(() => ctx.navigate("voice")))),
  );
  const gender = () => row(t("Voice"), null, segmented([["female", t("Female")], ["male", t("Male")]], prefs.voiceGender, (g) => void ctx.setPrefs((p) => (p.voiceGender = g))));
  if (prefs.voiceOutput === "kitten") {
    answers.append(
      divider(),
      row(t("Kitten, on your computer"),
        t("Says the answers here, without the internet. English only: answers in another language get the system voice. Downloaded once (about {0} MB).", { 0: number(status.models.kittenMb) }),
        modelsControl(ctx, () => ctx.navigate("voice"), KITTEN)),
      divider(),
      gender(),
    );
    if (status.models.kitten.state !== "ready") {
      answers.append(h("div", { class: "hub-warn", text: t("Until Kitten is downloaded, the system voice says the answers.") }));
    }
  } else if (prefs.voiceOutput === "elevenlabs") {
    answers.append(
      divider(),
      gender(),
      divider(),
      row(t("ElevenLabs key"), null, elevenKey()),
    );
  }
  page.append(answers);

  page.append(label(t("Privacy")));
  page.append(card(
    ...privacyNotes(status.ai, prefs.voiceOutput === "elevenlabs"),
    divider(),
    h("div", { class: "hub-note", text: t("An email is never sent by Ciel: your mail app opens with it written, and you send it.") }),
  ));
}

/** « OK Ciel » through the user's own Claude Code: the switch, and what Ciel found of it. */
function claudeCodeCard(agent: AgentStatus, on: boolean, set: (on: boolean) => void): HTMLElement {
  const found = !!agent.path;
  const state = !found
    ? t("Ciel can't find claude: install Claude Code's command line.")
    : !agent.signedIn
      ? t("Sign in to Claude Code first: run claude once in a terminal.")
      : t("Found: {version}.", { version: agent.version ?? "claude" });
  return card(
    row(t("Talk through your Claude Code"), t("« OK Ciel » becomes a conversation with Claude, on your own Claude Code and its plan. What acts asks for your OK in the island."), toggle(on, set)),
    divider(),
    h("div", { class: found && agent.signedIn ? "hub-note" : "hub-warn", text: state }),
    h("div", { class: "hub-note", text: t("What Claude reads to answer you goes to Anthropic, as in Claude Code. Ciel never sees your sign-in.") }),
  );
}

/** Where the words after « OK Ciel » go, with Claude or with a local model. */
function privacyNotes(ai: AiStatus, eleven: boolean): HTMLElement[] {
  const note = (text: string) => h("div", { class: "hub-note", text });
  if (ai.engine !== "local") {
    return [note(t("Your voice never leaves this computer: Parakeet turns it into text here. Only the words after « OK Ciel » go to Anthropic with your Claude key, and to ElevenLabs if you picked it for the answers."))];
  }
  const model = localName(ai);
  const notes = [
    note(staysHere(ai)
      ? t("Your voice never leaves this computer: Parakeet turns it into text here, and {model} answers here too.", { model })
      : t("Your voice never leaves this computer: Parakeet turns it into text here. Only the words after « OK Ciel » go to {url}, your model server.", { url: ai.url ?? "" })),
  ];
  if (eleven) notes.push(note(t("Only the answers go to ElevenLabs, to be said out loud.")));
  notes.push(note(t("Web search works with Claude only.")));
  return notes;
}
