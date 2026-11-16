// Canvas: what Ciel's brain drew for you, the last ten (src-tauri/src/canvas.rs),
// one shown big. A page is shown sealed off (core/canvas.ts) and taken down
// when you leave the page or hide the window, so it never runs unseen.

import { h } from "../../views/dom";
import { language, t } from "../../i18n/i18n";
import { Bridge, onEvent } from "../../core/bridge";
import { asMeta, hideDrawing, showDrawing, type DrawingMeta } from "../../core/canvas";
import { emptyState } from "../hero";
import type { HubContext } from "../shell";
import { button } from "../ui";

export function canvasPage(container: HTMLElement, ctx: HubContext) {
  const list = h("div", { class: "canvas-list" });
  const stage = h("div", { class: "canvas-big" });
  const body = h("div", { class: "canvas-page" }, list, stage);
  const clearAll = button(t("Remove all"), () => void Bridge.canvasClear().then(load), "danger", true);
  container.append(ctx.hero("canvas", t("Canvas"), t("What Ciel drew for you while you talked, the last ten."), [clearAll], true), body);

  let drawings: DrawingMeta[] = [];
  let current: string | null = null;
  const when = new Intl.DateTimeFormat(language(), { weekday: "short", hour: "2-digit", minute: "2-digit" });

  const show = (id: string | null) => {
    current = id;
    for (const row of list.children) row.classList.toggle("on", (row as HTMLElement).dataset.id === id);
    const drawing = drawings.find((d) => d.id === id);
    if (!drawing || document.hidden) return hideDrawing(stage);
    void showDrawing(stage, drawing);
  };

  const draw = () => {
    clearAll.style.display = drawings.length > 1 ? "" : "none";
    if (drawings.length === 0) {
      list.replaceChildren();
      hideDrawing(stage);
      body.replaceChildren(emptyState(t("Nothing drawn yet. Ask Ciel to show you something: a plan, a chart, your week."), "idle"));
      return;
    }
    if (!body.contains(list)) body.replaceChildren(list, stage);
    list.replaceChildren(...drawings.map((d) =>
      h("div", { class: "canvas-row", "data-id": d.id, onclick: () => show(d.id) },
        h("div", { class: "canvas-row-text" },
          h("span", { class: "canvas-row-title", text: d.title }),
          h("span", { class: "canvas-row-when", text: when.format(new Date(d.at * 1000)) })),
        h("button", {
          class: "hub-btn small canvas-row-remove", text: "×", title: t("Remove"),
          onclick: (e: Event) => {
            e.stopPropagation();
            void Bridge.canvasRemove(d.id).then(load);
          },
        }))));
    show(drawings.some((d) => d.id === current) ? current : drawings[0].id);
  };

  async function load() {
    drawings = ((await Bridge.canvasList()) ?? []).map(asMeta).filter((d): d is DrawingMeta => d !== null);
    draw();
  }

  // A new drawing comes first, and is the one shown.
  const unlisten = onEvent<unknown>("canvas", (payload) => {
    const drawing = asMeta(payload);
    if (!drawing) return;
    current = drawing.id;
    void load();
  });
  // A hidden window runs nothing: the page comes back when it is shown.
  const visibility = () => show(current);
  document.addEventListener("visibilitychange", visibility);
  ctx.onLeave = () => {
    hideDrawing(stage);
    document.removeEventListener("visibilitychange", visibility);
    void unlisten.then((stop) => stop());
  };
  void load();
}
