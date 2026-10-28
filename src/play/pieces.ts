// Ciel's chess pieces, drawn here: soft shapes on one plinth, white like his
// body, black like the night sky. 100x100, standing on y = 90.

const BASE = "M28 76 H72 L77 82 H23 Z M20 84 Q20 82 22 82 H78 Q80 82 80 84 V88 Q80 90 78 90 H22 Q20 90 20 88 Z";
const BODY = "M40 63 C40 69 34 73 30 76 H70 C66 73 60 69 60 63 Z";
const COLLAR = "M35 58 H65 Q67 58 67 60.5 Q67 63 65 63 H35 Q33 63 33 60.5 Q33 58 35 58 Z";

/** Each piece: its shape, then the lines drawn over it (eye, slit, mane). */
const SHAPES: Record<string, { fill: string; lines?: string; dots?: [number, number, number][] }> = {
  P: {
    fill: "M50 17 A13 13 0 1 1 49.9 17 Z M37 46 H63 Q65 46 64 48 L60 52 H40 L36 48 Q35 46 37 46 Z M42 52 C42 62 36 70 30 76 H70 C64 70 58 62 58 52 Z",
  },
  R: {
    fill: "M28 18 H38 V26 H45 V18 H55 V26 H62 V18 H72 V36 L67 41 H33 L28 36 Z M35 41 H65 L62 74 H38 Z",
    lines: "M36 48 H64",
  },
  B: {
    fill: `M50 21 C38 30 33 42 39 53 H61 C67 42 62 30 50 21 Z M35 53 H65 Q67 53 67 55.5 Q67 58 65 58 H35 Q33 58 33 55.5 Q33 53 35 53 Z M42 58 C42 66 36 72 31 76 H69 C64 72 58 66 58 58 Z`,
    lines: "M56 31 L46 42",
    dots: [[50, 15, 5]],
  },
  N: {
    fill: "M31 76 C30 64 40 58 45 50 C39 53 33 55 28 51 C23 47 25 41 31 37 L43 27 C45 21 49 17 53 15 L55 8 L60 15 C71 19 77 31 77 45 C77 59 70 67 70 76 Z",
    lines: "M62 22 C68 30 70 40 69 52",
    dots: [[49, 29, 2.6]],
  },
  Q: {
    fill: `M24 30 L35 58 H65 L76 30 L63 44 L58 24 L50 41 L42 24 L37 44 Z ${COLLAR} ${BODY}`,
    dots: [[24, 27, 4.5], [42, 21, 4.5], [58, 21, 4.5], [76, 27, 4.5]],
  },
  K: {
    fill: `M47 6 H53 V13 H60 V19 H53 V31 H47 V19 H40 V13 H47 Z M30 36 C30 27 43 26 50 34 C57 26 70 27 70 36 C70 46 63 53 61 58 H39 C37 53 30 46 30 36 Z ${COLLAR} ${BODY}`,
    lines: "M50 34 V52",
  },
};

const ink = { w: "#1B2130", b: "#C9D3E6" };

/** The piece as an SVG string ("P" a white pawn, "q" a black queen). */
export function pieceSvg(piece: string): string {
  const kind = piece.toUpperCase();
  const color = piece === kind ? "w" : "b";
  const shape = SHAPES[kind];
  const grad = `g${color}`;
  const stops = color === "w" ? ["#FFFFFF", "#E6EAF2", "#C3CAD8"] : ["#4A5470", "#262C3C", "#11141C"];
  const stroke = color === "w" ? "#1B2130" : "#05070B";
  const dots = (shape.dots ?? []).map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}" fill="${kind === "N" ? ink[color] : `url(#${grad})`}" stroke="${kind === "N" ? "none" : stroke}" stroke-width="2.4"/>`)
    .join("");
  return `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
<defs><linearGradient id="${grad}" x1="0" y1="0" x2="0.35" y2="1">
<stop offset="0" stop-color="${stops[0]}"/><stop offset="0.55" stop-color="${stops[1]}"/><stop offset="1" stop-color="${stops[2]}"/>
</linearGradient></defs>
<g stroke="${stroke}" stroke-width="2.4" stroke-linejoin="round">
<path d="${shape.fill} ${BASE}" fill="url(#${grad})"/>
${dots}
</g>
${shape.lines ? `<path d="${shape.lines}" fill="none" stroke="${ink[color]}" stroke-width="2.6" stroke-linecap="round"/>` : ""}
</svg>`;
}
