// Notifications in the island: when one arrives (core/notifications.ts), the
// folded island slides out with its app's icon and title beside Ciel, and the
// card shows under it — the app, the title, the text, Open and Dismiss —
// for a few seconds, longer while the pointer is on it. Several at once: the
// newest shows, with how many more came, and they all fold away together (they
// are still in Ciel's list, views/inbox.ts). Dictation goes first: a
// notification that arrives meanwhile waits for it to end. During a call the
// Notetaker records, the call keeps the folded island's row and only the
// answer key's cards show, under it; the others wait for the call to end.
// Theirs says Ciel is thinking, then the answer is written into it as it
// comes, in place and without announcing it again; whole, it has Copy, which
// puts it on the clipboard and leaves the card there.
//
// Open goes back to where it came from (Rust: the tab in Chrome, the mail in
// Gmail, the app); Dismiss only takes it out of the island and the list.

import { Bridge, onEvent } from "../core/bridge";
import {
  addNotification, applyUpdate, cardButtons, isAnswerKeyCard, keepsCard, MAX_UNSEEN, nextToShow, parseNotification,
  parseNotificationUpdate, showFor, timeSpan, type CielNotification,
} from "../core/notifications";
import { State } from "../core/state";
import { Tools } from "../tools/store";
import { language, t } from "../i18n/i18n";
import { noteIcon as appIcon, settle } from "../views/inbox";

export interface NotificationCard {
  /** Its app and title, beside Ciel in the folded island. */
  el: HTMLElement;
  /** The card, under the island. */
  pop: HTMLElement;
  /** A notification is showing. */
  readonly active: boolean;
  /** The card's clickable part, in island pixels; null when nothing shows. */
  hitBox(): { w: number; h: number } | null;
  /**
   * The island is folded and free (no dictation): the card may show. `inCall`:
   * a call the Notetaker records holds the row, and only the answer key's show.
   */
  sync(free: boolean, inCall?: boolean): void;
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

/** Where notifications come from: Rust's events, or a preview page. */
export type NotificationSource = (deliver: (raw: unknown) => void) => void;

const FROM_RUST: NotificationSource = (deliver) => void onEvent<unknown>("notification", deliver);
const UPDATES_FROM_RUST: NotificationSource = (deliver) => void onEvent<unknown>("notification-update", deliver);

export function buildNotificationCard(
  onChange: () => void,
  onArrive: (n: CielNotification) => void,
  source: NotificationSource = FROM_RUST,
  updates: NotificationSource = UPDATES_FROM_RUST,
): NotificationCard {
  let queue: CielNotification[] = [];
  /** The one showing, if any. */
  let showing: CielNotification | null = null;
  let free = false;
  let inCall = false;
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
    // 0 (Settings): it stays until opened or dismissed.
    const ms = showFor(showing, State.settings.notificationSeconds);
    if (ms === 0) return;
    timer = window.setTimeout(() => {
      timer = null;
      // Shown long enough: it folds away, with the ones that came with it.
      queue = [];
      showing = null;
      next();
    }, ms);
  }

  function act(n: CielNotification, action: string) {
    queue = queue.filter((q) => q.id !== n.id);
    if (showing?.id === n.id) showing = null;
    stopTimer();
    settle(n.id, action);
    next();
  }

  function render() {
    const n = free ? showing : null;
    // During a call, its strip keeps the row: the card shows under it alone.
    strip.classList.toggle("on", !!n && !inCall);
    pop.classList.toggle("on", !!n);
    // An answer is read whole: a wider card, scrolling past a few lines.
    pop.classList.toggle("answer", !!n && isAnswerKeyCard(n));
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
    // Its own buttons if it has some (the last one white), else Open.
    const choices = cardButtons(n, t("Open"));
    const buttons = choices.map((b, i) => {
      const button = own(el("button", i === choices.length - 1 ? "note-open" : "note-dismiss", b.label));
      button.addEventListener("click", () => {
        // An answer's Copy: Rust copies it, the button says so, the card stays.
        if (keepsCard(n, b.id)) {
          void Bridge.notificationAction(n.id, b.id);
          button.textContent = t("Copied");
          return;
        }
        act(n, b.id);
      });
      return button;
    });
    const dismiss = own(el("button", "note-dismiss", t("Dismiss")));
    dismiss.addEventListener("click", () => act(n, "dismiss"));
    pop.replaceChildren(
      el("div", "note-head", ""),
      el("div", "note-title", n.title),
      ...(n.time ? [el("div", "note-time", timeSpan(n.time, language()))] : []),
      ...(n.body ? [el("div", "note-body", n.body)] : []),
      el("div", "note-actions", ""),
    );
    const head = pop.firstElementChild as HTMLElement;
    head.append(appIcon(n, 18), el("span", "note-app", n.app));
    (pop.lastElementChild as HTMLElement).append(dismiss, ...buttons);
  }

  /** The newest waiting one shows, when the island is free. */
  function next() {
    if (!showing && free && queue.length) {
      showing = nextToShow(queue, inCall);
      if (showing) startTimer();
    }
    render();
    onChange();
  }

  source((raw) => {
    const n = parseNotification(raw);
    // Paused from the tray: Ciel says nothing.
    if (!n || State.paused) return;
    State.notifications = addNotification(State.notifications, n, MAX_UNSEEN);
    // Do not disturb (Tools → Controls): kept in the list, not shown, but
    // for the answer key's, which a press asked for.
    if (Tools.controls?.dnd && !isAnswerKeyCard(n)) return onChange();
    queue = addNotification(queue, n);
    // A newer one takes the place of the one showing.
    showing = null;
    onArrive(n);
    next();
  });

  // A card that says something new: in place, no sound, no new arrival.
  updates((raw) => {
    const u = parseNotificationUpdate(raw);
    if (!u) return;
    State.notifications = applyUpdate(State.notifications, u);
    queue = applyUpdate(queue, u);
    if (showing?.id !== u.id) return;
    showing = queue.find((q) => q.id === u.id) ?? applyUpdate([showing], u)[0];
    // How long it stays changed (the answer is whole, or a word took its place): counted from now.
    if (u.until !== undefined && free) startTimer();
    render();
    onChange();
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
    sync(isFree: boolean, call = false) {
      if (free === isFree && inCall === call) return;
      free = isFree;
      inCall = call;
      // A call began: what isn't the answer key's goes back to waiting.
      if (inCall && showing && !isAnswerKeyCard(showing)) {
        showing = null;
        stopTimer();
      }
      if (!free) stopTimer();
      else if (showing) startTimer();
      next();
    },
  };
}
