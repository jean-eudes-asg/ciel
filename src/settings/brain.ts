// Notch → Chat → Ciel's brain: who thinks for « OK Ciel », the dictation
// clean-up and the call summaries — Claude with the key above, or the local
// model connected below (src-tauri/src/ai.rs decides; this shows it).

import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { onEvent } from "../core/bridge";
import { FlowBridge } from "../core/flow";
import { NO_BRAIN, aiEngine, brainPrivacy, brainState, engineChoices, staysHere, type AiEngine } from "../core/brain";
import { statusDot } from "./parts";

/**
 * The section, and `refresh` to call when something it depends on changed
 * here (the Claude key). Settings saved anywhere refresh it on their own.
 */
export function brainSection(current: unknown, pick: (engine: AiEngine) => Promise<void>): { section: HTMLElement; refresh: () => void } {
  const dot = statusDot(false);
  const select = h("select", {}) as HTMLSelectElement;
  for (const [id, label] of engineChoices()) select.append(h("option", { value: id, text: label }));
  select.value = aiEngine(current);
  const explain = h("div", { class: "hint" });
  const state = h("div", { class: "hint" });
  const where = h("div", {});

  let drawn = 0;
  const refresh = async () => {
    const asked = ++drawn;
    const ai = (await FlowBridge.status())?.ai ?? { ...NO_BRAIN, pref: aiEngine(select.value) };
    if (asked !== drawn) return;
    select.value = ai.pref;
    dot.style.background = ai.engine === "off" ? "#f4505e" : "#22c55e";
    explain.textContent = ai.pref === "auto" ? t("Automatic: Claude when a key is saved, otherwise your local model.") : "";
    explain.style.display = ai.pref === "auto" ? "" : "none";
    state.textContent = brainState(ai);
    clear(where);
    const privacy = brainPrivacy(ai);
    if (privacy) where.append(h("div", { class: staysHere(ai) ? "notice ok" : "hint", text: privacy }));
    if (ai.engine === "local") where.append(h("div", { class: "hint", text: t("Web search works with Claude only.") }));
  };

  select.addEventListener("change", () => void pick(aiEngine(select.value)).then(refresh));
  const section = h(
    "section",
    {},
    h("h2", {}, dot, h("span", { text: t("Ciel's brain") })),
    h("div", { class: "hint", text: t("Who thinks for « OK Ciel », the dictation clean-up and the call summaries. The chat keeps its own choice, above the chat box.") }),
    h("div", { class: "row" }, h("label", { text: t("Engine") }), select),
    explain,
    state,
    where,
  );
  void onEvent("settings-changed", () => {
    if (section.isConnected) void refresh();
  });
  void refresh();
  return { section, refresh: () => void refresh() };
}
