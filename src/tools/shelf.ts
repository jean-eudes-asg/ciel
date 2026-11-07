// Tools → Shelf: the files dropped on Ciel, kept a week — a drop is how they
// get here (island.ts swallow). Ask the chat about one, drag one (or the ones
// picked) out into any app, show it in its folder, copy it; a double click
// opens it.
// Windows drags them itself (src-tauri/src/shelf.rs); on Linux the page hands
// WebKitGTK a text/uri-list, which the desktop takes like any file drag.

import { Bridge } from "../core/bridge";
import { HOST_OS } from "../core/pills";
import { State } from "../core/state";
import type { ShelfFile } from "../core/tools";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import type { ViewActions } from "../views/views";
import { language, t, tl, tn } from "../i18n/i18n";
import { Tools } from "./store";
import { ago, roundButton, size } from "./ui";
import type { ToolPanel } from "./view";

/** ".pdf" → "PDF", what the tile shows when there is no picture. */
function ext(name: string): string {
  const m = /\.([a-z0-9]{1,5})$/i.exec(name);
  return m ? m[1].toUpperCase() : "";
}

/** file:///C:/a%20b.png, file:///home/me/a.png */
export function fileUri(path: string): string {
  const slashed = path.replace(/\\/g, "/");
  const abs = /^[a-zA-Z]:\//.test(slashed) ? `/${slashed}` : slashed;
  return `file://${abs.split("/").map((s) => encodeURIComponent(s).replace(/%3A/gi, ":")).join("/")}`;
}

export function buildShelf(actions: ViewActions): ToolPanel {
  const picked = new Set<string>();
  const count = h("span", { class: "tool-count" });
  const copyAll = h("button", { class: "tool-link", text: tl("Copy"), onclick: () => copy(selection()) });
  const removeAll = h("button", { class: "tool-link", text: tl("Remove"), onclick: () => remove(selection()) });
  const head = h("div", { class: "tool-row shelf-head" },
    h("span", { class: "tool-label", text: tl("Shelf") }), count, h("div", { class: "grow" }), copyAll, removeAll);
  const grid = h("div", { class: "shelf-grid" });
  const empty = h("div", { class: "tool-intro small" },
    h("div", { class: "shelf-drop-art" }, svg(ICONS.shelf, 26, { stroke: 1.6 })),
    h("div", { class: "tool-intro-text", text: tl("Drop files on Ciel: they wait here for a week, to ask him about or drag into any app.") }),
  );
  const toast = h("div", { class: "tool-toast" });
  const el = h("div", { class: "tool tool-shelf" }, head, grid, empty, toast);

  let toastTimer = 0;
  function flash(msg: string) {
    toast.textContent = msg;
    toast.classList.add("on");
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => toast.classList.remove("on"), 1300);
  }

  /** The picked files, or all of them when none is picked. */
  function selection(): string[] {
    const paths = Tools.shelf.map((f) => f.path);
    const chosen = paths.filter((p) => picked.has(p));
    return chosen.length ? chosen : paths;
  }

  function copy(paths: string[]) {
    actions.blip();
    void Bridge.shelfCopy(paths).then((ok) => ok && flash(tn("File copied", "{count} files copied", paths.length)));
  }

  function remove(paths: string[]) {
    actions.blip();
    for (const p of paths) picked.delete(p);
    void Bridge.shelfRemove(paths).then(() => Tools.refreshShelf());
  }

  /** The chat, with this file to talk about (as right after its drop). */
  function ask(file: ShelfFile) {
    actions.blip();
    State.droppedFile = { name: file.name, path: file.path };
    State.droppedMore = 0;
    State.promptContext = { kind: "file", name: file.name, path: file.path };
    State.chatHistory = [];
    void Bridge.chatReset();
    actions.setView("prompt");
  }

  function startDrag(e: DragEvent, file: ShelfFile) {
    const paths = picked.has(file.path) ? selection() : [file.path];
    Tools.draggingOut = true;
    window.setTimeout(() => (Tools.draggingOut = false), 30_000);
    if (HOST_OS === "windows") {
      // Windows' own drag carries real files; the page's would carry a link.
      e.preventDefault();
      void Bridge.shelfDrag(paths).finally(() => (Tools.draggingOut = false));
      return;
    }
    const dt = e.dataTransfer;
    if (!dt) return;
    dt.effectAllowed = "copy";
    dt.setData("text/uri-list", paths.map(fileUri).join("\r\n"));
    dt.setData("text/plain", paths.join("\n"));
  }

  function tile(file: ShelfFile): HTMLElement {
    const art = file.preview
      ? h("img", { class: "shelf-art", src: file.preview, alt: "", draggable: "false" })
      : h("div", { class: "shelf-art doc" }, svg(ICONS.doc, 18), h("b", { text: ext(file.name) }));
    const actionsRow = h("div", { class: "shelf-actions" },
      roundButton(ICONS.bubble, tl("Ask a question about it"), () => ask(file), 0),
      roundButton(ICONS.folder, tl("Show in folder"), () => void Bridge.shelfReveal(file.path)),
      roundButton(ICONS.copy, tl("Copy"), () => copy([file.path]), 0),
      roundButton(ICONS.trash, tl("Remove"), () => remove([file.path])),
    );
    const el = h("div", {
      class: picked.has(file.path) ? "shelf-tile picked" : "shelf-tile",
      draggable: "true",
      title: `${file.name}\n${size(file.size, language())} · ${ago(file.at, language())}\n${t("Drag it into any app")}`,
      onclick: () => {
        if (picked.has(file.path)) picked.delete(file.path);
        else picked.add(file.path);
        key = "";
        State.notify();
      },
      ondblclick: () => void Bridge.shelfOpen(file.path),
    }, art, h("div", { class: "shelf-name", text: file.name }), h("div", { class: "shelf-size", text: size(file.size, language()) }), actionsRow);
    el.addEventListener("dragstart", (e) => startDrag(e as DragEvent, file));
    el.addEventListener("dragend", () => (Tools.draggingOut = false));
    return el;
  }

  let key = "";

  return {
    el,
    shown() {
      void Tools.refreshShelf();
    },
    sync() {
      const files = Tools.shelf;
      for (const p of [...picked]) if (!files.some((f) => f.path === p)) picked.delete(p);
      const next = `${language()}|${files.map((f) => `${f.path}:${picked.has(f.path)}`).join(",")}`;
      empty.style.display = files.length ? "none" : "";
      head.style.display = files.length ? "" : "none";
      grid.style.display = files.length ? "" : "none";
      count.textContent = picked.size ? t("{picked} of {count}", { picked: picked.size, count: files.length }) : String(files.length);
      copyAll.textContent = picked.size ? t("Copy") : t("Copy all");
      removeAll.textContent = picked.size ? t("Remove") : t("Remove all");
      if (next === key) return;
      key = next;
      grid.replaceChildren(...files.slice(0, 24).map(tile));
    },
  };
}
