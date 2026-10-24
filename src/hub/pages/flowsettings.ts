// Flow → Settings: the key, where the pill is, the microphone, the voice
// model, the Notetaker's summaries, the window and the history. Port of
// FlowHubSettings.swift.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { onEvent } from "../../core/bridge";
import { FlowBridge, type Edge, type FlowKey, type ModelState, type ModelsStatus } from "../../core/flow";
import { localName, staysHere } from "../../core/brain";
import type { HubContext } from "../shell";
import { button, card, divider, keyKeycap, keycap, label, number, row, segmented, select, toggle } from "../ui";

/** A model downloaded on a click: where its state is, and how to get or remove it. */
export interface ModelKind {
  pick: (models: ModelsStatus) => ModelState;
  download: () => Promise<unknown>;
  remove: () => Promise<unknown>;
  removeLabel: () => string;
}

/** Parakeet and the voices model: dictation, the Notetaker and « OK Ciel » hear with them. */
const PARAKEET: ModelKind = {
  pick: (m) => m.parakeet,
  download: () => FlowBridge.download(),
  remove: () => FlowBridge.removeModels(),
  removeLabel: () => t("Remove the models"),
};

/** Kitten: the voice « OK Ciel » can answer in, on this computer. */
export const KITTEN: ModelKind = {
  pick: (m) => m.kitten,
  download: () => FlowBridge.downloadKitten(),
  remove: () => FlowBridge.removeKitten(),
  removeLabel: () => t("Remove the voice"),
};

/** Kokoro: « OK Ciel »'s natural voices, on this computer. */
export const KOKORO: ModelKind = {
  pick: (m) => m.kokoro,
  download: () => FlowBridge.downloadKokoro(),
  remove: () => FlowBridge.removeKokoro(),
  removeLabel: () => t("Remove the voice"),
};

/** Download / progress / installed, for a model (Parakeet unless told). Redraws itself. */
export function modelsControl(ctx: HubContext, onReady?: () => void, kind: ModelKind = PARAKEET): HTMLElement {
  const el = h("div", { style: "display:flex;align-items:center;gap:8px" });
  let confirming = false;
  const draw = (models: ModelsStatus) => {
    clear(el);
    const m = kind.pick(models);
    if (m.state === "missing") {
      el.append(button(t("Download"), () => void kind.download(), "primary"));
    } else if (m.state === "downloading") {
      const bar = h("div", { class: "hub-progress" }, h("i", { style: `width:${Math.round(m.fraction * 100)}%` }));
      el.append(bar, h("span", { class: "hub-note", text: `${Math.round(m.fraction * 100)} %` }));
    } else if (m.state === "failed") {
      el.append(h("span", { class: "hub-note", text: m.error, title: m.error, style: "max-width:220px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" }));
      el.append(button(t("Retry"), () => void kind.download()));
    } else if (confirming) {
      el.append(
        button(kind.removeLabel(), () => {
          confirming = false;
          void kind.remove();
        }, "danger", true),
        button(t("Cancel"), () => {
          confirming = false;
          draw(ctx.status.models);
        }, "", true),
      );
    } else {
      const ok = h("button", { class: "hub-link", style: "color:var(--hub-green)", text: `✓ ${t("Installed")}`, title: kind.removeLabel() });
      ok.addEventListener("click", () => {
        confirming = true;
        draw(ctx.status.models);
      });
      el.append(ok);
    }
  };
  draw(ctx.status.models);
  void onEvent<ModelsStatus>("speech-models", (models) => {
    if (!el.isConnected) return;
    const wasReady = kind.pick(ctx.status.models).state === "ready";
    ctx.status.models = models;
    draw(models);
    if (!wasReady && kind.pick(models).state === "ready") onReady?.();
  });
  return el;
}

export async function flowSettingsPage(page: HTMLElement, ctx: HubContext) {
  const status = await ctx.refresh();
  const prefs = status.prefs;
  page.append(ctx.hero("flowsettings", t("Settings"), null));

  // ── Keys ──
  const keys: [FlowKey, string][] = [
    ["right_ctrl", t("Right {0}", { 0: "Ctrl" })],
    ["right_alt", t("Right {0}", { 0: "Alt" })],
    ["right_shift", t("Right {0}", { 0: "Shift" })],
    ["left_ctrl_super", status.platform === "windows" ? "Ctrl + Win" : "Ctrl + Super"],
  ];
  const edges: [Edge, string][] = [["island", t("Island")], ["bottom", t("Bottom")], ["left", t("Left")], ["right", t("Right")]];
  page.append(label(t("Keys")));
  const keysCard = card(
    row(t("Hold a key to dictate anywhere"), null, toggle(prefs.enabled, (on) => void ctx.setPrefs((p) => (p.enabled = on)))),
    divider(),
    status.keys === "wayland"
      ? row(t("Hold to dictate"), null, keyKeycap("wayland", status.platform))
      : row(t("Hold to dictate"), prefs.key === "right_alt" ? t("On a keyboard with AltGr, the right Alt types characters: pick another key.") : null,
        select(keys, prefs.key, (key) => void ctx.setPrefs((p) => (p.key = key)).then(() => ctx.navigate("flowsettings")))),
    divider(),
    row(t("Where dictation shows"), t("The island at the top of the screen, or the pill on an edge."),
      segmented(edges, prefs.edge, (edge) => void ctx.setPrefs((p) => (p.edge = edge)))),
    divider(),
    row(t("Hands-free"), t("Tap twice, speak as long as you like, tap once to paste."), keyKeycap(status.keys === "wayland" ? "wayland" : prefs.key, status.platform, true)),
    divider(),
    row(t("Paste the last dictation again"), t("When the words went to the wrong place."), button(t("Paste again"), () => void FlowBridge.pasteAgain(), "", true)),
    divider(),
    row(t("Cancel a dictation"), null, keycap(t("Esc"))),
  );
  if (status.keys === "wayland") {
    keysCard.append(h("div", {
      class: "hub-warn",
      text: t("On Wayland, the desktop holds the dictation key for Ciel: Ctrl+Alt+V, or the keys you picked when it asked. The first paste asks once to let Ciel type for you."),
    }));
  } else if (status.keys !== "ok") {
    keysCard.append(h("div", { class: "hub-warn", text: t("Ciel can't watch the keyboard here: use the pill's microphone to dictate.") }));
  }
  page.append(keysCard);

  // ── Microphone ──
  page.append(label(t("Microphone")));
  const mics: [string, string][] = [["", t("System default")], ...status.mics.map((m): [string, string] => [m.name, m.name])];
  if (prefs.mic && !mics.some(([id]) => id === prefs.mic)) mics.push([prefs.mic, prefs.mic]);
  page.append(card(row(t("Microphone"), t("The one Ciel listens to for dictation and calls."), select(mics, prefs.mic, (mic) => void ctx.setPrefs((p) => (p.mic = mic))))));

  // ── Recognition ──
  page.append(label(t("Recognition")));
  page.append(card(
    row(t("Parakeet, on your computer"), t("The most accurate, offline, 25 languages. Downloaded once (about {0} MB).", { 0: number(status.models.downloadMb) }), modelsControl(ctx)),
    h("div", { class: "hub-note", text: t("Your voice never leaves this computer: Parakeet turns it into text here, and the recording is dropped as soon as it is pasted.") }),
  ));

  // ── Notetaker ──
  page.append(label(t("Notetaker")));
  // "claude" is the stored value for "summaries on": Ciel's brain writes them, Claude or the local model.
  const ai = status.ai;
  const local = ai.engine === "local" ? localName(ai) : null;
  const summaryNote = !local
    ? t("With Claude, the transcript (never the audio) goes to Anthropic with your key from Notch → Chat.")
    : staysHere(ai)
      ? t("With {model}, the transcript stays on this computer.", { model: local })
      : t("With {model}, the transcript (never the audio) goes to {url}, your model server.", { model: local, url: ai.url ?? "" });
  page.append(card(
    row(t("Summaries"), null, select([["claude", local ?? t("Claude (your key)")], ["off", t("No summary")]], prefs.summaryEngine, (engine) => void ctx.setPrefs((p) => (p.summaryEngine = engine)))),
    h("div", { class: "hub-note", text: summaryNote }),
    divider(),
    h("div", { class: "hub-note", text: t("Everything stays on this computer. Tell the people in the call that you are recording.") }),
  ));

  // ── App ──
  page.append(label(t("App")));
  page.append(card(
    row(t("Open this window when you open Ciel"), t("Not when Ciel starts with the computer."), toggle(prefs.openAtLaunch, (on) => void ctx.setPrefs((p) => (p.openAtLaunch = on)))),
  ));

  // ── Privacy ──
  page.append(label(t("Privacy")));
  const keep: [string, string][] = [["0", t("Don't keep")], ["7", t("7 days")], ["30", t("30 days")], ["365", t("1 year")], ["-1", t("Forever")]];
  const confirmRow = h("div", { class: "hub-row" });
  const erase = () => {
    clear(confirmRow);
    confirmRow.append(
      button(t("Erase the history"), () => void FlowBridge.clear(false), "", true),
      button(t("Erase everything"), () => {
        clear(confirmRow);
        confirmRow.append(
          h("span", { class: "hub-note", style: "flex:1", text: t("Erase the history and the statistics?") }),
          button(t("Erase everything"), () => {
            void FlowBridge.clear(true);
            erase();
          }, "danger", true),
          button(t("Cancel"), erase, "", true),
        );
      }, "", true),
    );
  };
  erase();
  page.append(card(
    row(t("Keep the history"), null, select(keep, String(prefs.keepDays), (v) => void ctx.setPrefs((p) => (p.keepDays = Number(v))))),
    divider(),
    confirmRow,
    h("div", { class: "hub-note", text: t("The history, the statistics and your notes stay on this computer, in Ciel's folder. Dictations are never saved as audio; a call's audio is deleted as soon as its transcript is written.") }),
  ));
}
