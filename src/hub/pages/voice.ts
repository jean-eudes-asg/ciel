// « OK Ciel »: say it, then ask anything. Turning it on, how Ciel answers
// out loud, and what leaves the computer. Port of the Mac's Settings → Voice.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { Bridge } from "../../core/bridge";
import type { VoiceOutput } from "../../core/flow";
import { localName, staysHere, type AiStatus } from "../../core/brain";
import type { HubContext } from "../shell";
import { button, card, divider, label, row, segmented, toggle } from "../ui";
import { modelsControl } from "./flowsettings";

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
  page.append(label(t("Listening")));
  const main = card(
    row(t("Listen for « OK Ciel »"), t("The microphone stays open; only what follows « OK Ciel » goes further."),
      toggle(prefs.voice, (on) => void ctx.setPrefs((p) => (p.voice = on)).then(() => ctx.navigate("voice")))),
    divider(),
    row(t("Parakeet, on your computer"), t("Hears « OK Ciel » here, without the internet."), modelsControl(ctx, () => ctx.navigate("voice"))),
  );
  if (prefs.voice && status.ai.engine === "off") {
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

  page.append(label(t("Answers")));
  const outputs: [VoiceOutput, string][] = [["system", t("System voice")], ["elevenlabs", "ElevenLabs"], ["off", t("Text only")]];
  const answers = card(
    row(t("Ciel answers out loud"), null, segmented(outputs, prefs.voiceOutput, (v) => void ctx.setPrefs((p) => (p.voiceOutput = v)).then(() => ctx.navigate("voice")))),
  );
  if (prefs.voiceOutput === "elevenlabs") {
    answers.append(
      divider(),
      row(t("Voice"), null, segmented([["female", t("Female")], ["male", t("Male")]], prefs.voiceGender, (g) => void ctx.setPrefs((p) => (p.voiceGender = g)))),
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
