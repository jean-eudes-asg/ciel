// The weather's little pictures, drawn in SVG on a 32 grid: the sun or the
// moon, Ciel-like clouds, rain, snow, a bolt, fog. Colour, unlike the icons.

import type { Sky } from "../core/weather";

const NS = "http://www.w3.org/2000/svg";

function el<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>): SVGElementTagNameMap[K] {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

const CLOUD = "M9 26h14.5a5.5 5.5 0 0 0 .7-11 7.5 7.5 0 0 0-14.4 1.9A4.6 4.6 0 0 0 9 26z";
const SMALL_CLOUD = "M10.5 22h11a4.2 4.2 0 0 0 .5-8.4 5.8 5.8 0 0 0-11.1 1.4 3.5 3.5 0 0 0-.4 7z";

function sun(cx: number, cy: number, r: number): SVGGElement {
  const g = el("g", {});
  g.append(el("circle", { cx, cy, r, fill: "#FFC94D" }));
  for (let i = 0; i < 8; i++) {
    const a = (i / 8) * Math.PI * 2;
    g.append(el("line", {
      x1: cx + Math.cos(a) * (r + 2.2), y1: cy + Math.sin(a) * (r + 2.2),
      x2: cx + Math.cos(a) * (r + 4.6), y2: cy + Math.sin(a) * (r + 4.6),
      stroke: "#FFC94D", "stroke-width": 2, "stroke-linecap": "round",
    }));
  }
  return g;
}

function moon(cx: number, cy: number, r: number): SVGPathElement {
  return el("path", {
    d: `M${cx + r * 0.35} ${cy - r} a${r} ${r} 0 1 0 ${r * 0.65} ${r * 1.55} a${r * 0.8} ${r * 0.8} 0 0 1 -${r * 0.65} -${r * 1.55}z`,
    fill: "#E9E3C9",
  });
}

export function weatherIcon(sky: Sky, isDay: boolean, size = 32): SVGSVGElement {
  const svg = el("svg", { viewBox: "0 0 32 32", width: size, height: size, "aria-hidden": "true", class: "weather-icon" });
  const light = isDay ? sun : moon;
  const cloud = (d: string, fill: string) => el("path", { d, fill });
  switch (sky) {
    case "clear":
      svg.append(light(16, 16, isDay ? 6.5 : 8));
      break;
    case "partly":
      svg.append(light(isDay ? 12 : 13, isDay ? 11 : 11, isDay ? 5 : 6), cloud(CLOUD, "#E6EBF2"));
      break;
    case "cloudy":
      svg.append(cloud("M6 18h10a4 4 0 0 0 .4-8 5.4 5.4 0 0 0-10.4 1.2A3.3 3.3 0 0 0 6 18z", "#8C95A3"), cloud(CLOUD, "#D5DCE6"));
      break;
    case "fog":
      svg.append(cloud(SMALL_CLOUD, "#AEB6C2"));
      for (const [y, x1, x2] of [[25, 7, 25], [28.5, 10, 22]] as const) {
        svg.append(el("line", { x1, y1: y, x2, y2: y, stroke: "#AEB6C2", "stroke-width": 2, "stroke-linecap": "round" }));
      }
      break;
    case "drizzle":
    case "rain": {
      svg.append(cloud(SMALL_CLOUD, sky === "rain" ? "#AFBACB" : "#C9D2DE"));
      const drops = sky === "rain" ? [[12, 25], [17, 27], [22, 25]] : [[13, 25.5], [20, 26.5]];
      for (const [x, y] of drops) {
        svg.append(el("line", { x1: x + 0.8, y1: y - 2.6, x2: x - 0.6, y2: y + 1.4, stroke: "#7FB4EA", "stroke-width": 2, "stroke-linecap": "round" }));
      }
      break;
    }
    case "snow":
      svg.append(cloud(SMALL_CLOUD, "#D9E0EA"));
      for (const [x, y] of [[12, 25.5], [17, 28], [22, 25.5]]) svg.append(el("circle", { cx: x, cy: y, r: 1.5, fill: "#FFFFFF" }));
      break;
    case "storm":
      svg.append(cloud(SMALL_CLOUD, "#7C8696"));
      svg.append(el("path", { d: "M17 21.5h-3.2l-1.8 5h2.6l-1.2 4.5 5.2-6.4h-2.8l1.2-3.1z", fill: "#FDE047" }));
      break;
  }
  return svg;
}
