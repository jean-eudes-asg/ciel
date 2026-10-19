// Notifications in the island: when one arrives (core/notifications.ts), the
// folded island slides out with its app's icon and title beside Ciel, and the
// card shows under it — the app, the title, the text, Open and Dismiss —
// for a few seconds, longer while the pointer is on it. Several at once: the
// newest shows, with how many more wait. Dictation goes first: a notification
// that arrives meanwhile waits for it to end.
//
// Open goes back to where it came from (Rust: the tab in Chrome, the mail in
// Gmail); Dismiss only takes it out of the island.

import { Bridge, onEvent } from "../core/bridge";
import { addNotification, parseNotification, SHOW_MS, type CielNotification } from "../core/notifications";
import { State } from "../core/state";
import { t } from "../i18n/i18n";

export interface NotificationCard {
  /** Its app and title, beside Ciel in the folded island. */
  el: HTMLElement;
  /** The card, under the island. */
  pop: HTMLElement;
  /** A notification is showing. */
  readonly active: boolean;
  /** The card's clickable part, in island pixels; null when nothing shows. */
  hitBox(): { w: number; h: number } | null;
  /** The island is folded and free (no dictation): the card may show. */
  sync(free: boolean): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

/** A press that stays its own: a press on the island opens it. */
function own(e: HTMLElement): HTMLElement {
  e.addEventListener("mousedown", (ev) => ev.stopPropagation());
  return e;
}

function appIcon(n: CielNotification, size: number): HTMLElement {
  const box = el("span", "note-icon");
  box.style.width = box.style.height = `${size}px`;
  if (n.icon) {
    const img = el("img");
    img.src = n.icon;
    img.alt = "";
    img.draggable = false;
    box.append(img);
  } else {
    box.textContent = (n.app.trim()[0] ?? "•").toUpperCase();
  }
  return box;
}

export function buildNotificationCard(onChange: () => void, onArrive: (n: CielNotification) => void): NotificationCard {
  let queue: CielNotification[] = [];
  /** The one showing, if any. */
  let showing: CielNotification | null = null;
  let free = false;
  let hovered = false;
  let timer: number | null = null;

  const strip = el("div", "note-strip");
  const pop = own(el("div", "note-pop"));
  pop.addEventListener("mouseenter", () => {
    hovered = true;
    stopTimer();
  });
  pop.addEventListener("mouseleave", () => {
    hovered = false;
    startTimer();
  });

  function stopTimer() {
    if (timer != null) window.clearTimeout(timer);
    timer = null;
  }

  function startTimer() {
    stopTimer();
    if (!showing || hovered) return;
    timer = window.setTimeout(() => {
      timer = null;
      // Shown long enough: it folds away; the next waiting one, if any, comes.
      if (showing) queue = queue.filter((q) => q.id !== showing!.id);
      showing = null;
      next();
    }, SHOW_MS);
  }

  function act(n: CielNotification, action: "open" | "dismiss") {
    queue = queue.filter((q) => q.id !== n.id);
    if (showing?.id === n.id) showing = null;
    stopTimer();
    void Bridge.notificationAction(n.id, action);
    next();
  }

  function render() {
    const n = free ? showing : null;
    strip.classList.toggle("on", !!n);
    pop.classList.toggle("on", !!n);
    if (!n) {
      strip.replaceChildren();
      pop.replaceChildren();
      return;
    }
    const more = queue.length - 1;
    strip.replaceChildren(
      appIcon(n, 16),
      el("span", "note-strip-title", n.title),
      ...(more > 0 ? [el("span", "note-more", `+${more}`)] : []),
    );
    const open = own(el("button", "note-open", t("Open")));
    open.addEventListener("click", () => act(n, "open"));
    const dismiss = own(el("button", "note-dismiss", t("Dismiss")));
    dismiss.addEventListener("click", () => act(n, "dismiss"));
    pop.replaceChildren(
      el("div", "note-head", ""),
      el("div", "note-title", n.title),
      ...(n.body ? [el("div", "note-body", n.body)] : []),
      el("div", "note-actions", ""),
    );
    const head = pop.firstElementChild as HTMLElement;
    head.append(appIcon(n, 18), el("span", "note-app", n.app));
    (pop.lastElementChild as HTMLElement).append(dismiss, open);
  }

  /** The newest waiting one shows, when the island is free. */
  function next() {
    if (!showing && free && queue.length) {
      showing = queue[0];
      startTimer();
    }
    render();
    onChange();
  }

  void onEvent<unknown>("notification", (raw) => {
    const n = parseNotification(raw);
    // Paused from the tray: Ciel says nothing.
    if (!n || State.paused) return;
    queue = addNotification(queue, n);
    // A newer one takes the place of the one showing.
    showing = null;
    onArrive(n);
    next();
  });

  return {
    el: strip,
    pop,
    get active() {
      return free && showing != null;
    },
    hitBox() {
      if (!(free && showing)) return null;
      return { w: pop.offsetWidth, h: pop.offsetTop + pop.offsetHeight };
    },
    sync(isFree: boolean) {
      if (free === isFree) return;
      free = isFree;
      if (!free) stopTimer();
      else if (showing) startTimer();
      next();
    },
  };
}
