// An agent's reply, in the sky island: instead of opening the island on its
// card, what it said comes up in a bubble under the folded island, beside
// that agent's own little cloud. It stays a few seconds (longer while the
// pointer is on it), and a click opens the island on the card.

import { createMiniBot, pruneMiniBots } from "../cloud/minibots";
import type { AgentTask } from "../core/state";
import { h } from "../views/dom";

/** How long a reply stays under the island. */
const SHOW_MS = 8000;

export interface ReplyBubble {
  el: HTMLElement;
  /** A reply is showing: the folded island stays out for it. */
  readonly active: boolean;
  show(task: AgentTask, text: string): void;
  hide(): void;
}

export function buildReplyBubble(open: (taskId: string) => void, changed: () => void): ReplyBubble {
  const who = h("span", { class: "reply-who" });
  const cloud = h("span", { class: "reply-cloud" });
  const text = h("p", { class: "reply-text" });
  const el = h("button", { class: "reply-bubble" }, cloud, h("span", { class: "reply-body" }, who, text));
  let taskId: string | null = null;
  let timer = 0;

  const hide = () => {
    window.clearTimeout(timer);
    if (!taskId) return;
    taskId = null;
    el.classList.remove("on");
    changed();
  };
  const wait = () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(hide, SHOW_MS);
  };

  el.addEventListener("mouseenter", () => window.clearTimeout(timer));
  el.addEventListener("mouseleave", () => taskId && wait());
  el.addEventListener("click", (e) => {
    e.stopPropagation();
    const id = taskId;
    hide();
    if (id) open(id);
  });

  return {
    el,
    get active() {
      return taskId != null;
    },
    show(task, said) {
      taskId = task.id;
      who.textContent = task.name;
      who.style.color = task.color;
      text.textContent = said;
      cloud.replaceChildren(createMiniBot(task, 20));
      pruneMiniBots();
      // A second reply while the bubble shows: its words come up anew.
      text.classList.remove("line");
      void text.offsetWidth;
      text.classList.add("line");
      el.classList.add("on");
      wait();
      changed();
    },
    hide,
  };
}
