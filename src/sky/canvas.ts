// Canvases that work in the island, the live page and the tests: an
// OffscreenCanvas where there is one, else a detached <canvas>.

export type AnyCanvas = HTMLCanvasElement | OffscreenCanvas;

export function makeCanvas(width: number, height: number): AnyCanvas {
  const w = Math.max(1, Math.round(width));
  const h = Math.max(1, Math.round(height));
  if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(w, h);
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

/**
 * The 2D context of any canvas, typed as the DOM's so the painters (and
 * BotEngine) take both kinds.
 */
export function context(canvas: AnyCanvas): CanvasRenderingContext2D {
  const ctx = canvas.getContext("2d") as CanvasRenderingContext2D | null;
  if (!ctx) throw new Error("no 2D context");
  return ctx;
}

/** A canvas encoded as PNG bytes. */
export async function toPng(canvas: AnyCanvas): Promise<Uint8Array> {
  let blob: Blob | null;
  if ("convertToBlob" in canvas) {
    blob = await canvas.convertToBlob({ type: "image/png" });
  } else {
    blob = await new Promise<Blob | null>((resolve) => (canvas as HTMLCanvasElement).toBlob(resolve, "image/png"));
  }
  if (!blob) throw new Error("the picture could not be encoded");
  return new Uint8Array(await blob.arrayBuffer());
}
