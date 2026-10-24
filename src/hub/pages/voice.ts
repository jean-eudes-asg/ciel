// « OK Ciel »: say it, then ask anything. Turning it on, how Ciel answers
// out loud, and what leaves the computer. Port of the Mac's Settings → Voice.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { Bridge } from "../../core/bridge";
import { FlowBridge, type AgentStatus, type FlowPrefs, type VoiceOutput } from "../../core/flow";
import { localName, staysHere, type AiStatus } from "../../core/brain";
import type { HubContext } from "../shell";
import { button, card, divider, label, number, row, segmented, toggle } from "../ui";
import { KITTEN, KOKORO, modelsControl } from "./flowsettings";

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
  // Another name, for a voice the model writes « Ciel » wrong for (wake.rs).
  const name = h("input", {
    class: "hub-input", type: "text", value: prefs.wakeName ?? "", placeholder: t("A name"),
    autocomplete: "off", spellcheck: "false", maxlength: "40", style: "width:180px",
  }) as HTMLInputElement;
  name.addEventListener("change", () => void ctx.setPrefs((p) => (p.wakeName = name.value.trim())));
  const main = card(
    row(t("Listen for « OK Ciel »"), t("The microphone stays open; only what follows « OK Ciel » goes further."),
      toggle(prefs.voice, (on) => void ctx.setPrefs((p) => (p.voice = on)).then(() => ctx.navigate("voice")))),
    divider(),
    row(t("Also answers to"), t("Another name that wakes Ciel when you say it first: « Raphael, what time is it? ». Pick one that dictation writes right."), name),
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
  const outputs: [VoiceOutput, string][] = [["system", t("System voice")], ["kokoro", "Kokoro"], ["kitten", "Kitten"], ["elevenlabs", "ElevenLabs"], ["server", t("Voice server")], ["off", t("Text only")]];
  const answers = card(
    row(t("Ciel answers out loud"), null, segmented(outputs, prefs.voiceOutput, (v) => void ctx.setPrefs((p) => (p.voiceOutput = v)).then(() => ctx.navigate("voice")))),
  );
  const gender = () => row(t("Voice"), null, segmented([["female", t("Female")], ["male", t("Male")]], prefs.voiceGender, (g) => void ctx.setPrefs((p) => (p.voiceGender = g))));
  if (prefs.voiceOutput === "kokoro") {
    // The voices voice/kokoro.rs offers, by the names Kokoro gives them.
    const voices: [string, string][] = [["af_heart", "Heart"], ["af_bella", "Bella"], ["af_nicole", "Nicole"], ["am_michael", "Michael"], ["am_fenrir", "Fenrir"], ["am_puck", "Puck"]];
    const pick = h("select", { class: "hub-input", style: "width:180px" }) as HTMLSelectElement;
    for (const [id, name] of voices) pick.append(h("option", { value: id, text: name }));
    pick.value = voices.some(([id]) => id === prefs.kokoroVoice) ? prefs.kokoroVoice! : "af_heart";
    pick.addEventListener("change", () => void ctx.setPrefs((p) => (p.kokoroVoice = pick.value)));
    answers.append(
      divider(),
      row(t("Kokoro, on your computer"),
        t("Natural voices, made here without the internet: English, and French in its own voice. Downloaded once (about {0} MB).", { 0: number(status.models.kokoroMb) }),
        modelsControl(ctx, () => ctx.navigate("voice"), KOKORO)),
      divider(),
      row(t("Voice"), t("Heart, Bella and Nicole are women's voices; Michael, Fenrir and Puck, men's."), pick),
    );
    if (status.models.kokoro.state !== "ready") {
      answers.append(h("div", { class: "hub-warn", text: t("Until Kokoro is downloaded, the system voice says the answers.") }));
    }
  } else if (prefs.voiceOutput === "kitten") {
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
  } else if (prefs.voiceOutput === "server") {
    answers.append(divider(), ...serverRows(ctx, prefs));
  }
  page.append(answers);

  page.append(label(t("Privacy")));
  page.append(card(
    ...privacyNotes(status.ai, prefs.voiceOutput === "elevenlabs"),
    divider(),
    h("div", { class: "hub-note", text: t("An email is never sent by Ciel: your mail app opens with it written, and you send it.") }),
  ));
}

/**
 * A voice server of the user's, on OpenAI's speech API (VoiceStudio and
 * others): its address, and the voice, from the ones it lists.
 */
function serverRows(ctx: HubContext, prefs: FlowPrefs): HTMLElement[] {
  const url = h("input", {
    class: "hub-input", type: "text", value: prefs.voiceServerUrl ?? "http://127.0.0.1:3900",
    autocomplete: "off", spellcheck: "false", style: "width:220px",
  }) as HTMLInputElement;
  const voice = h("select", { class: "hub-input", style: "width:220px" }) as HTMLSelectElement;
  const problem = h("div", { class: "hub-warn" });
  problem.hidden = true;
  const load = async () => {
    let chosen = prefs.voiceServerVoice ?? "";
    clear(voice);
    voice.append(h("option", { value: "", text: t("The server's own voice") }));
    problem.hidden = true;
    try {
      const voices = await FlowBridge.voiceServerVoices(url.value.trim());
      for (const v of voices) voice.append(h("option", { value: v.id, text: v.name }));
      // The server answered without it: the voice is gone, its own one speaks.
      if (chosen && !voices.some((v) => v.id === chosen)) {
        chosen = "";
        void ctx.setPrefs((p) => (p.voiceServerVoice = prefs.voiceServerVoice = ""));
      }
    } catch (err) {
      problem.textContent = String(err);
      problem.hidden = false;
      // Kept while the server is closed.
      if (chosen) voice.append(h("option", { value: chosen, text: chosen }));
    }
    voice.value = chosen;
  };
  url.addEventListener("change", () => void ctx.setPrefs((p) => (p.voiceServerUrl = url.value.trim())).then(load));
  voice.addEventListener("change", () => void ctx.setPrefs((p) => (p.voiceServerVoice = prefs.voiceServerVoice = voice.value)));
  void load();
  return [
    row(t("Address"), t("A voice server on this computer or your network that speaks OpenAI's speech API, like VoiceStudio. When it doesn't answer, the system voice speaks."), url),
    divider(),
    row(t("Voice"), null, voice),
    problem,
  ];
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
