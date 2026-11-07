// Tools → Clipboard: what was copied, newest first, the pinned ones on top.
// A click pastes it in the app you were in; each kind has its own little
// tricks (a link loses its trackers, a colour shows its other spellings, a
// sum its result, JSON its layout). Off until the user turns it on here.

import { Bridge, IS_TAURI } from "../core/bridge";
import { State } from "../core/state";
import type { ClipItem } from "../core/tools";
import {
  classify, cleanLink, colorForms, counts, evalMath, formatNumber, linkHost, matches, parseColor, preview,
  prettyJson, transform, type Transform,
} from "../core/clip";
import { clock } from "../core/timers";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import type { ViewActions } from "../views/views";
import { language, t, tl } from "../i18n/i18n";
import { Tools } from "./store";
import { ago, roundButton, typingField } from "./ui";
import type { ToolPanel } from "./view";

interface Trick {
  label: string;
  title?: string;
  text: () => string;
}

/** What the row offers besides pasting it as it is. */
function tricks(item: ClipItem): Trick[] {
  if (item.kind !== "text") return [];
  const text = item.text;
  const kind = classify(text);
  const lang = language();
  const out: Trick[] = [];
  if (kind === "link") {
    const clean = cleanLink(text);
    if (clean) out.push({ label: t("Clean link"), title: t("Without what follows you around"), text: () => clean });
  }
  if (kind === "color") {
    const c = parseColor(text);
    if (c) {
      const f = colorForms(c);
      out.push({ label: "HEX", text: () => f.hex }, { label: "RGB", text: () => f.rgb }, { label: "HSL", text: () => f.hsl });
    }
  }
  if (kind === "math") {
    const v = evalMath(text);
    if (v != null) out.push({ label: `= ${formatNumber(v, lang)}`, title: t("Paste the result"), text: () => String(v) });
  }
  if (kind === "json") {
    if (prettyJson(text) !== text) out.push({ label: t("Lay out"), text: () => transform(text, "json") });
  }
  if (kind === "text" || kind === "email" || kind === "code") {
    const cases: [string, Transform][] = [["AA", "upper"], ["aa", "lower"], ["Aa", "title"]];
    if (/\p{L}/u.test(text) && kind === "text") for (const [label, how] of cases) out.push({ label, text: () => transform(text, how, lang) });
    if (/[ \t]{2,}|^\s|\s$|\n{3,}/.test(text)) out.push({ label: t("Tidy spaces"), text: () => transform(text, "trim") });
  }
  return out;
}

function kindIcon(item: ClipItem): HTMLElement {
  if (item.kind === "image" && item.thumb) {
    return h("img", { class: "clip-thumb", src: item.thumb, alt: "" });
  }
  if (item.kind === "files") return h("span", { class: "clip-kind" }, svg(ICONS.folder, 13, { stroke: 1.8 }));
  const kind = classify(item.text);
  if (kind === "color") {
    const c = parseColor(item.text);
    if (c) return h("span", { class: "clip-swatch", style: `background:rgba(${c.r},${c.g},${c.b},${c.a})` });
  }
  if (kind === "link") return h("span", { class: "clip-kind" }, svg(ICONS.link, 13, { stroke: 1.8 }));
  if (kind === "math") return h("span", { class: "clip-kind", text: "=" });
  if (kind === "code" || kind === "json") return h("span", { class: "clip-kind mono", text: "{ }" });
  return h("span", { class: "clip-kind" }, svg(ICONS.doc, 12));
}

function title(item: ClipItem): string {
  if (item.kind === "image") return t("Picture · {width}×{height}", { width: item.width, height: item.height });
  if (item.kind === "files") {
    const files = item.text.split("\n");
    const first = files[0].split(/[\\/]/).pop() ?? files[0];
    return files.length > 1 ? t("{name} and {count} more", { name: first, count: files.length - 1 }) : first;
  }
  return preview(item.text, 160);
}

export function buildClipboard(actions: ViewActions): ToolPanel {
  const search = typingField(h("input", {
    class: "tool-input",
    type: "search",
    spellcheck: "false",
    placeholder: tl("Search what you copied"),
  }) as HTMLInputElement);
  const clearBtn = h("button", {
    class: "tool-link",
    text: tl("Clear"),
    title: tl("Forget everything but the pinned ones"),
    onclick: () => {
      actions.blip();
      void Bridge.clipboardClear();
    },
  });
  const offBtn = h("button", {
    class: "tool-link",
    text: tl("Turn off"),
    onclick: () => {
      actions.blip();
      Tools.setPrefs({ clipboard: false });
    },
  });
  const head = h("div", { class: "tool-row" }, search, clearBtn, offBtn);
  const list = h("div", { class: "tool-list clip-list" });
  const toast = h("div", { class: "tool-toast" });

  const intro = h("div", { class: "tool-intro" },
    h("div", { class: "tool-intro-title", text: tl("Ciel can keep what you copy") }),
    h("div", { class: "tool-intro-text", text: tl("Text, links, pictures and files, on this computer only and until Ciel quits; what you pin stays. Password managers' copies are never kept, and one-time codes go after two minutes.") }),
    h("button", {
      class: "btn primary",
      onclick: () => {
        actions.blip();
        Tools.setPrefs({ clipboard: true });
      },
    }, h("span", { text: tl("Keep what I copy") })),
  );
  const blind = h("div", { class: "tool-hint", text: tl("This desktop doesn't tell apps what is copied. Install wl-clipboard and Ciel will see it.") });

  const el = h("div", { class: "tool tool-clipboard" }, intro, h("div", { class: "clip-on" }, head, blind, list), toast);
  const on = el.querySelector(".clip-on") as HTMLElement;

  let toastTimer = 0;
  function flash(msg: string) {
    toast.textContent = msg;
    toast.classList.add("on");
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => toast.classList.remove("on"), 1300);
  }

  function paste(item: ClipItem) {
    actions.blip();
    void Bridge.clipboardUse(item.id, true).then((ok) => {
      if (ok) actions.collapse();
      else flash(t("Copied"));
    });
  }

  function useText(text: string, pasteIt: boolean) {
    actions.blip();
    void Bridge.clipboardPut(text, pasteIt).then((ok) => {
      if (ok && pasteIt) actions.collapse();
      else if (ok) flash(t("Copied"));
    });
  }

  const expiries = new Map<number, HTMLElement>();

  function row(item: ClipItem): HTMLElement {
    const meta = h("div", { class: "clip-meta" });
    const when = ago(item.at, language());
    meta.append(h("span", { text: item.app ? `${item.app} · ${when}` : when }));
    if (item.kind === "text") {
      const kind = classify(item.text);
      if (kind === "link") {
        const host = linkHost(item.text);
        if (host) meta.append(h("span", { class: "clip-host", text: host }));
      }
      const c = counts(item.text);
      if (c.words >= 12) meta.append(h("span", { text: t("{count} words", { count: c.words }) }));
    }
    if (item.expires) {
      const gone = h("span", { class: "clip-expires" });
      expiries.set(item.id, gone);
      meta.append(gone);
    }
    const chips = h("div", { class: "clip-tricks" });
    for (const trick of tricks(item)) {
      chips.append(h("button", {
        class: "tool-chip small",
        text: trick.label,
        title: trick.title ?? t("Paste it like this"),
        onclick: (e: Event) => {
          e.stopPropagation();
          useText(trick.text(), true);
        },
      }));
    }
    const buttons = h("div", { class: "clip-actions" },
      roundButton(ICONS.copy, tl("Copy"), () => {
        actions.blip();
        void Bridge.clipboardUse(item.id, false).then((ok) => ok && flash(t("Copied")));
      }, 0),
      roundButton(ICONS.pin, item.pinned ? tl("Unpin") : tl("Pin"), () => {
        actions.blip();
        void Bridge.clipboardPin(item.id, !item.pinned);
      }),
      roundButton(ICONS.trash, tl("Remove"), () => {
        actions.blip();
        void Bridge.clipboardRemove(item.id);
      }),
    );
    const text = h("div", { class: item.kind === "text" && /code|json/.test(classify(item.text)) ? "clip-text mono" : "clip-text", text: title(item) });
    const r = h("div", {
      class: `clip-row${item.pinned ? " pinned" : ""}`,
      role: "button",
      tabindex: "0",
      title: t("Paste"),
      onclick: () => paste(item),
    }, kindIcon(item), h("div", { class: "clip-body" }, text, meta, chips), buttons);
    if (item.pinned) r.append(h("span", { class: "clip-pin-mark" }, svg(ICONS.pin, 9, { stroke: 2.4 })));
    return r;
  }

  search.addEventListener("input", () => {
    key = "";
    State.notify();
  });
  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      const first = shown()[0];
      if (first) paste(first);
    }
  });

  function shown(): ClipItem[] {
    const now = Date.now();
    const items = Tools.clip.items.filter((i) => !i.expires || i.expires > now).filter((i) => matches(i, search.value));
    return [...items.filter((i) => i.pinned), ...items.filter((i) => !i.pinned)];
  }

  let key = "";
  const empty = h("div", { class: "tool-empty" });

  return {
    el,
    shown() {
      void Bridge.clipboardState().then((s) => {
        if (s) {
          Tools.clip = s;
          State.notify();
        }
      });
    },
    sync() {
      const enabled = Tools.prefs.clipboard;
      intro.style.display = enabled ? "none" : "";
      on.style.display = enabled ? "" : "none";
      blind.style.display = enabled && Tools.clip.on && !Tools.clip.watching && IS_TAURI ? "" : "none";
      clearBtn.style.display = Tools.clip.items.some((i) => !i.pinned) ? "" : "none";
      if (!enabled) return;
      const items = shown();
      const next = `${search.value}|${language()}|${items.map((i) => `${i.id}:${i.pinned}:${i.at}`).join(",")}`;
      if (next === key) return;
      key = next;
      expiries.clear();
      empty.textContent = search.value ? t("Nothing copied matches.") : t("Copy something: it shows here.");
      list.replaceChildren(...(items.length ? items.slice(0, 40).map(row) : [empty]));
    },
    tick() {
      if (!expiries.size) return false;
      const now = Date.now();
      for (const item of Tools.clip.items) {
        const gone = expiries.get(item.id);
        if (gone && item.expires) gone.textContent = t("goes in {time}", { time: clock(Math.max(0, item.expires - now)) });
      }
      return true;
    },
    focus() {
      search.focus();
    },
  };
}
