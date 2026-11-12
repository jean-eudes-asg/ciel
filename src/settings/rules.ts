// Settings → Agents → Remembered answers: every rule "Don't ask again" has
// made on a permission card (src-tauri/src/rules.rs), with what it lets
// through, for which agent and in which project folder — and a button to take
// it back. A rule removed here stops answering at once, even for a request
// already on its way.

import { Bridge } from "../core/bridge";
import { asRule, ruleLabel, type Rule } from "../core/rules";
import { agentName } from "../island/agents";
import { clear, h } from "../views/dom";
import { t } from "../i18n/i18n";

/** The agent as the island names it; the relay gives Claude Code no tag. */
function who(rule: Rule): string {
  return rule.agent ? agentName(rule.agent) : "Claude Code";
}

export function rulesSection(): HTMLElement {
  const list = h("div", { style: "display:flex;flex-direction:column;gap:10px" });

  async function load() {
    const rules = ((await Bridge.rulesList()) ?? []).map(asRule).filter((r): r is Rule => r !== null);
    draw(rules);
  }

  function draw(rules: Rule[]) {
    clear(list);
    if (rules.length === 0) {
      list.append(h("div", { class: "hint", text: t("Nothing remembered yet.") }));
      return;
    }
    for (const rule of rules) {
      list.append(
        h("div", { class: "row" },
          h("div", { style: "display:flex;flex-direction:column;gap:2px;min-width:0" },
            h("span", { class: "path", text: ruleLabel(rule) }),
            h("span", { class: "hint", text: [who(rule), rule.tool, rule.cwd].join(" · ") }),
          ),
          h("span", { class: "spacer" }),
          h("button", {
            class: "danger",
            text: t("Remove"),
            onclick: async () => {
              await Bridge.rulesRemove(rule.id);
              void load();
            },
          }),
        ),
      );
    }
    if (rules.length > 1) {
      list.append(
        h("div", { class: "row" },
          h("span", { class: "spacer" }),
          h("button", {
            class: "danger",
            text: t("Remove all"),
            onclick: async () => {
              await Bridge.rulesClear();
              void load();
            },
          }),
        ),
      );
    }
  }

  void load();
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("Remembered answers") })),
    h("div", {
      class: "hint",
      text: t("When you click Don't ask again on a permission card, Ciel allows the same kind of request from that agent in that project at once, and leaves a line on the session. Remove a rule and Ciel asks again."),
    }),
    list,
  );
}
