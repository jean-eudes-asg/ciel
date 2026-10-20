// The notifications Ciel keeps (core/notifications.ts): every one that came,
// until it is opened, dismissed or cleared. Folded, the island shows their
// apps' icons beside Ciel; a press there opens the island on the list. Open,
// the list: a press on a row opens it where it came from (Rust: the app, the
// tab in Chrome, the mail in Gmail), its ✕ only takes it out.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import type { CielNotification } from "../core/notifications";
import { t, tl } from "../i18n/i18n";
import { h } from "./dom";
import type { ViewActions, ViewHost } from "./views";

/** Open or dismiss: it leaves the list, and Rust does the rest. */
export function settle(id: string, action: "open" | "dismiss") {
  State.notifications = State.notifications.filter((n) => n.id !== id);
  void Bridge.notificationAction(id, action);
  State.notify();
}

export function noteIcon(n: CielNotification, size: number): HTMLElement {
  const box = h("span", { class: "note-icon" });
  box.style.width = box.style.height = `${size}px`;
  if (n.icon) {
    const img = h("img", { src: n.icon, alt: "", draggable: "false" });
    box.append(img);
  } else {
    box.textContent = (n.app.trim()[0] ?? "•").toUpperCase();
  }
  return box;
}

/** "now", "5 min. ago", "2 hr. ago", in the interface's language. */
export function ago(at: number, now = Date.now()): string {
  const rtf = new Intl.RelativeTimeFormat(document.documentElement.lang || undefined, { numeric: "auto", style: "short" });
  const min = Math.floor((now - at) / 60_000);
  if (min < 1) return rtf.format(0, "second");
  if (min < 60) return rtf.format(-min, "minute");
  if (min < 24 * 60) return rtf.format(-Math.floor(min / 60), "hour");
  return rtf.format(-Math.floor(min / (24 * 60)), "day");
}

// ── Folded: the apps waiting, beside Ciel ─────────────────────────────────────

export interface NoteStack {
  el: HTMLElement;
  sync(show: boolean): void;
}

/** Up to three apps' icons, newest in front, and how many wait. */
export function buildNoteStack(): NoteStack {
  const el = h("div", { id: "note-stack" });
  // The island opens on the list: the flag is read as it opens, before its own handler.
  const want = () => {
    State.openOnNotifications = true;
  };
  el.addEventListener("mousedown", want);
  el.addEventListener("mouseenter", () => {
    if (State.settings.openOnHover) want();
  });
  let key = "";
  return {
    el,
    sync(show) {
      const list = State.notifications;
      const on = show && list.length > 0;
      el.classList.toggle("on", on);
      if (!on) return;
      const apps: CielNotification[] = [];
      for (const n of list) {
        if (!apps.some((a) => a.app === n.app)) apps.push(n);
        if (apps.length === 3) break;
      }
      const next = `${apps.map((a) => a.id).join("|")}#${list.length}`;
      if (next === key) return;
      key = next;
      el.replaceChildren(
        h("span", { class: "note-stack-icons" }, ...apps.map((n) => noteIcon(n, 16))),
        h("span", { class: "note-count", text: list.length > 99 ? "99+" : String(list.length) }),
      );
    },
  };
}

// ── Open: the list ────────────────────────────────────────────────────────────

/**
 * How much room the list lacks to show every row (+), or has to spare (−), in
 * px: the island grows or shrinks by it (island.ts, layout.ts inboxHeight).
 */
export function listShortfall(list: HTMLElement): number {
  const last = list.lastElementChild as HTMLElement | null;
  const pad = parseFloat(getComputedStyle(list).paddingBottom) || 0;
  const natural = last ? last.getBoundingClientRect().bottom - list.getBoundingClientRect().top + list.scrollTop + pad : pad;
  return natural - list.clientHeight;
}

/** `onRows`: the rows changed, the island measures again (shortfall) once at rest. */
export function buildInbox(actions: ViewActions, onRows: () => void = () => {}): ViewHost {
  const count = h("span", { class: "inbox-count" });
  const clearBtn = h("button", {
    class: "inbox-clear",
    text: tl("Clear"),
    onclick: () => {
      actions.blip();
      for (const n of State.notifications) void Bridge.notificationAction(n.id, "dismiss");
      State.notifications = [];
      State.notify();
    },
  });
  const list = h("div", { class: "inbox-list" });
  const empty = h("div", { class: "inbox-empty", text: tl("You're all caught up.") });
  const el = h(
    "div",
    { class: "view inbox" },
    h("div", { class: "card" },
      h("div", { class: "inbox-body" },
        h("div", { class: "inbox-head" }, h("span", { class: "inbox-title", text: tl("Notifications") }), count, h("div", { class: "grow" }), clearBtn),
        list,
        empty,
      ),
    ),
  );

  const times = new Map<string, HTMLElement>();
  let key = "";

  function row(n: CielNotification): HTMLElement {
    const when = h("span", { class: "inbox-when", text: ago(n.at) });
    times.set(n.id, when);
    const dismiss = h("button", { class: "inbox-dismiss", title: tl("Dismiss"), text: "✕" });
    dismiss.addEventListener("click", (e) => {
      e.stopPropagation();
      settle(n.id, "dismiss");
    });
    const r = h(
      "div",
      { class: "inbox-row", role: "button", tabindex: "0", title: t("Open") },
      noteIcon(n, 30),
      h("div", { class: "inbox-text" },
        h("div", { class: "inbox-meta" }, h("span", { class: "inbox-app", text: n.app }), when),
        h("div", { class: "inbox-row-title", text: n.title }),
        ...(n.body ? [h("div", { class: "inbox-row-body", text: n.body })] : []),
      ),
      dismiss,
    );
    r.addEventListener("click", () => {
      actions.blip();
      settle(n.id, "open");
    });
    return r;
  }

  return {
    el,
    shortfall: () => listShortfall(list),
    sync() {
      const all = State.notifications;
      count.textContent = all.length ? String(all.length) : "";
      clearBtn.style.visibility = all.length ? "visible" : "hidden";
      empty.style.display = all.length ? "none" : "";
      const next = all.map((n) => n.id).join("|");
      if (next !== key) {
        key = next;
        times.clear();
        list.replaceChildren(...all.map(row));
        // Every one in sight: the island makes the room, or gives it back.
        onRows();
      } else {
        const now = Date.now();
        for (const n of all) {
          const w = times.get(n.id);
          if (w) w.textContent = ago(n.at, now);
        }
      }
    },
  };
}
