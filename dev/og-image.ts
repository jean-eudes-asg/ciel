// The website's social card (site/public/og.jpg, 1200×630): a golden-hour
// sky with Ciel, from the app's own renderer, and the tagline. Not shipped.

import "@fontsource-variable/nunito/wght.css";
import { DEFAULT_WALLPAPER } from "../src/sky/prefs";
import { sampleSky, sceneSpec } from "../src/sky/scenes";
import { SkyRenderer } from "../src/sky/render";

const W = 1200;
const H = 630;
const params = new URLSearchParams(location.search);
const canvas = document.getElementById("og") as HTMLCanvasElement;
canvas.width = W;
canvas.height = H;
const ctx = canvas.getContext("2d")!;

const spec = sceneSpec(
  new Date(2026, 9, 18, 17, 40),
  "idle",
  { ...DEFAULT_WALLPAPER, cielX: 0.25, cielY: 0.5, cielSize: 0.54 },
  { choice: { theme: "golden", phase: "golden" }, sky: sampleSky("golden") },
);
spec.seed = Number(params.get("seed") ?? 11);
new SkyRenderer().render(ctx, spec, 40_000, { still: true });

const FONT = `"Nunito Variable", ui-rounded, system-ui, sans-serif`;
await document.fonts.load(`900 130px "Nunito Variable"`);
await document.fonts.load(`700 38px "Nunito Variable"`);

const x = 590;
ctx.save();
// A soft glow of the sky's own light behind the words.
const halo = ctx.createRadialGradient(x + 280, 330, 20, x + 280, 330, 440);
halo.addColorStop(0, "rgba(255,250,240,0.5)");
halo.addColorStop(1, "rgba(255,250,240,0)");
ctx.fillStyle = halo;
ctx.fillRect(x - 220, 0, W - x + 220, H);
ctx.restore();

ctx.fillStyle = "#13213f";
ctx.textBaseline = "alphabetic";
ctx.font = `900 150px ${FONT}`;
ctx.fillText("Ciel", x - 6, 310);
ctx.fillStyle = "#24314f";
// The tagline on two lines, as large as fits.
const lines = ["A little cloud that keeps an eye", "on your AI coding agents."];
let size = 40;
for (; size > 24; size--) {
  ctx.font = `750 ${size}px ${FONT}`;
  if (Math.max(...lines.map((l) => ctx.measureText(l).width)) <= W - x - 44) break;
}
lines.forEach((line, i) => ctx.fillText(line, x, 372 + i * size * 1.28));
ctx.font = `700 22px ${FONT}`;
ctx.fillStyle = "rgba(19,33,63,0.7)";
ctx.fillText("Free and open source · Windows and Linux", x, 500);

document.title = "ready";
