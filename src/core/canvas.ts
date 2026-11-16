// What « OK Ciel »'s brain drew (src-tauri/src/canvas.rs): small in the island,
// big on the Canvas page of the Ciel window.
//
// Markdown is rendered here, by Ciel's own renderer, which builds elements and
// never takes HTML. A page, which may hold scripts, never enters Ciel's own
// document: it is shown in a frame sandboxed to scripts only, served by Ciel
// under its own scheme with a policy that lets it reach nothing. It is taken
// down as soon as it is out of sight, so a live page never runs unseen.

import { Bridge } from "./bridge";
import { h } from "../views/dom";
import { renderMarkdown } from "../views/markdown";

export interface DrawingMeta {
  id: string;
  title: string;
  kind: "html" | "markdown";
  /** Seconds since 1970. */
  at: number;
}

export interface Drawing extends DrawingMeta {
  body: string;
}

/** A drawing as Rust describes it, or null. */
export function asMeta(value: unknown): DrawingMeta | null {
  if (!value || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  if (typeof v.id !== "string" || !/^[0-9a-z]{1,32}$/i.test(v.id)) return null;
  if (v.kind !== "html" && v.kind !== "markdown") return null;
  return {
    id: v.id,
    title: typeof v.title === "string" ? v.title : "",
    kind: v.kind,
    at: typeof v.at === "number" ? v.at : 0,
  };
}

/** The frame a page is shown in: scripts only, and nothing else. */
export function sealedFrame(url: string, title: string): HTMLIFrameElement {
  const frame = document.createElement("iframe");
  // No allow-same-origin, allow-popups, allow-forms, allow-modals or
  // allow-top-navigation: the page can't reach Ciel, open anything or leave.
  frame.setAttribute("sandbox", "allow-scripts");
  frame.setAttribute("referrerpolicy", "no-referrer");
  // No camera, microphone, screen, place or clipboard for it.
  frame.setAttribute("allow", "");
  frame.className = "canvas-frame";
  frame.title = title;
  frame.src = url;
  return frame;
}

/**
 * Shows `meta` in `into`, in place of what was there. A later call, or
 * `hideDrawing`, wins over one still loading.
 */
export async function showDrawing(into: HTMLElement, meta: DrawingMeta): Promise<void> {
  const token = `${meta.id}:${Date.now()}:${Math.random()}`;
  into.dataset.showing = token;
  if (meta.kind === "html") {
    const url = Bridge.canvasUrl(meta.id);
    into.replaceChildren(...(url ? [sealedFrame(url, meta.title)] : []));
    return;
  }
  const drawing = await Bridge.canvasGet(meta.id);
  if (into.dataset.showing !== token) return;
  const box = h("div", { class: "canvas-md" });
  renderMarkdown(box, drawing?.body ?? "");
  into.replaceChildren(box);
}

/** Takes the drawing down: a page's scripts stop with its frame. */
export function hideDrawing(into: HTMLElement): void {
  delete into.dataset.showing;
  into.replaceChildren();
}
