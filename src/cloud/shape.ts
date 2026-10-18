// Ciel's silhouette: five puffs on a flat-bottomed base. Everything is measured
// in units of the body radius R, centre at (0, 0), y pointing down. One source
// for the island (engine.ts), the launch greeting (greeting.ts) and the app icon
// (scripts/gen-icons.mjs), so this file stays free of DOM types: Node runs it too.

type Circle = readonly [cx: number, cy: number, r: number];

/** The puffs, crown first, then the shoulders and the feet. */
export const PUFFS: readonly Circle[] = [
  [0.0, -0.3, 0.56],
  [-0.56, -0.1, 0.43],
  [0.56, -0.05, 0.41],
  [-0.72, 0.3, 0.36],
  [0.72, 0.33, 0.34],
];

/** The flat base the puffs sit on, an ellipse. */
export const BASE = { cy: 0.36, ax: 0.82, ay: 0.48 } as const;

/** Half the outline's width and height, and how far its middle sits below (0, 0). */
export const EXTENT = { hw: 1.07, hh: 0.85, dy: -0.01 } as const;

/** How far the outline reaches from (0, 0) along the ray (cos a, sin a). */
function reach(ca: number, sa: number): number {
  let t = 0;
  for (const [cx, cy, r] of PUFFS) {
    const b = ca * cx + sa * cy;
    const disc = b * b - (cx * cx + cy * cy - r * r);
    if (disc >= 0) t = Math.max(t, b + Math.sqrt(disc));
  }
  // The base, as the unit circle once x and y are divided by its axes.
  const dx = ca / BASE.ax;
  const dy = sa / BASE.ay;
  const cy = BASE.cy / BASE.ay;
  const dd = dx * dx + dy * dy;
  const b = dy * cy;
  const disc = b * b - dd * (cy * cy - 1);
  if (disc >= 0) t = Math.max(t, (b + Math.sqrt(disc)) / dd);
  return t;
}

export const OUTLINE_STEPS = 144;

/**
 * reach() at OUTLINE_STEPS + 1 evenly spaced angles (the last repeats the
 * first), lightly smoothed so the dips between the puffs are soft, not sharp.
 */
export const OUTLINE: readonly number[] = (() => {
  const n = OUTLINE_STEPS;
  const raw = Array.from({ length: n }, (_, i) => {
    const a = (i / n) * Math.PI * 2;
    return reach(Math.cos(a), Math.sin(a));
  });
  const k = 1;
  const out = raw.map((_, i) => {
    let s = 0;
    for (let j = -k; j <= k; j++) s += raw[(i + j + n) % n];
    return s / (2 * k + 1);
  });
  out.push(out[0]);
  return out;
})();

/** The outline's radius at angle `a`, between two samples. */
export function outlineAt(a: number): number {
  const n = OUTLINE_STEPS;
  const u = ((((a / (Math.PI * 2)) % 1) + 1) % 1) * n;
  const i = Math.floor(u);
  const f = u - i;
  return OUTLINE[i] * (1 - f) + OUTLINE[i + 1] * f;
}

/** True when (x, y), in units of R, is inside the cloud. */
export function insideCloud(x: number, y: number): boolean {
  return Math.hypot(x, y) <= outlineAt(Math.atan2(y, x));
}
