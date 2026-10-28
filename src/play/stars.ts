// Constellations, the quiet puzzle of the Games page: pairs of stars of one
// colour in a night sky, to join with lines that never cross. Joining them
// all solves it; filling every point of the sky on the way is perfect.
//
// Made, not stored: one line through every point of the grid (a snake,
// shuffled by "backbite" steps until it wanders), cut into pieces; each
// piece's two ends are a pair. So there is always at least one way.

export interface Sky {
  size: number;
  /** Each pair's two points, cells numbered row by row. */
  pairs: [number, number][];
}

const neighbours = (size: number, i: number): number[] => {
  const r = Math.floor(i / size);
  const c = i % size;
  const out: number[] = [];
  if (r > 0) out.push(i - size);
  if (r < size - 1) out.push(i + size);
  if (c > 0) out.push(i - 1);
  if (c < size - 1) out.push(i + 1);
  return out;
};

export const adjacent = (size: number, a: number, b: number) => neighbours(size, a).includes(b);

/** A line through every cell, wandering. */
export function wander(size: number, random = Math.random): number[] {
  let path: number[] = [];
  for (let r = 0; r < size; r++) {
    for (let c = 0; c < size; c++) path.push(r * size + (r % 2 ? size - 1 - c : c));
  }
  for (let k = 0; k < size * size * 40; k++) {
    if (random() < 0.5) path.reverse();
    const end = path[path.length - 1];
    const options = neighbours(size, end).filter((n) => n !== path[path.length - 2]);
    const n = options[Math.floor(random() * options.length)];
    // Join the end to n, and break n's link to the cell after it.
    const at = path.indexOf(n);
    path = path.slice(0, at + 1).concat(path.slice(at + 1).reverse());
  }
  return path;
}

export const sizeFor = (level: number) => Math.min(9, 5 + Math.floor((level - 1) / 3));

export function sky(level: number, random = Math.random): Sky & { lines: number[][] } {
  const size = sizeFor(level);
  const path = wander(size, random);
  const lines: number[][] = [];
  let at = 0;
  while (at < path.length) {
    const left = path.length - at;
    let len = 3 + Math.floor(random() * (size * 1.4 - 2));
    // Never a last piece too short to be a pair.
    if (left - len < 3) len = left;
    lines.push(path.slice(at, at + len));
    at += len;
  }
  return { size, pairs: lines.map((l) => [l[0], l[l.length - 1]]), lines };
}

/** Each pair joined by its line, and how much of the sky the lines cover. */
export function progress(s: Sky, lines: number[][]): { joined: number; filled: number } {
  let joined = 0;
  const used = new Set<number>();
  s.pairs.forEach(([a, b], k) => {
    const l = lines[k] ?? [];
    for (const i of l) used.add(i);
    const ends = l.length > 1 && ((l[0] === a && l[l.length - 1] === b) || (l[0] === b && l[l.length - 1] === a));
    if (ends && l.every((cell, j) => j === 0 || adjacent(s.size, l[j - 1], cell))) joined++;
  });
  return { joined, filled: used.size / (s.size * s.size) };
}
