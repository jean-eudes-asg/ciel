// Clouds, the sliding puzzle of the Games page: a 6x6 patch of sky where
// clouds drift only along their length, and Ciel (two cells wide, on the
// third row) has to reach the sun on the right. A move is one cloud slid any
// distance. Puzzles are made, not stored: a random sky of clouds, every
// arrangement it can reach, and from those the one farthest from the sun,
// as near to the level's number of moves as that sky allows.

export const SIZE = 6;
export const EXIT_ROW = 2;

export interface Cloud {
  /** Its top-left cell. */
  r: number;
  c: number;
  len: number;
  /** Lies up and down. */
  v: boolean;
}

export interface Puzzle {
  /** Ciel first. */
  clouds: Cloud[];
  /** The fewest moves that solve it. */
  moves: number;
}

/** Where each cloud is along its line: its column, or its row for an upright one. */
type Pos = number[];

const along = (cl: Cloud) => (cl.v ? cl.r : cl.c);
const place = (cl: Cloud, p: number): Cloud => (cl.v ? { ...cl, r: p } : { ...cl, c: p });
const key = (pos: Pos) => String.fromCharCode(...pos);

function grid(clouds: Cloud[], pos: Pos): Int8Array {
  const g = new Int8Array(SIZE * SIZE).fill(-1);
  clouds.forEach((cl, i) => {
    for (let k = 0; k < cl.len; k++) {
      const r = cl.v ? pos[i] + k : cl.r;
      const c = cl.v ? cl.c : pos[i] + k;
      g[r * SIZE + c] = i;
    }
  });
  return g;
}

/** How far cloud `i` may slide back and forth: its lowest and highest place. */
export function room(clouds: Cloud[], pos: Pos, i: number): [number, number] {
  const g = grid(clouds, pos);
  const cl = clouds[i];
  const free = (p: number) => {
    const r = cl.v ? p : cl.r;
    const c = cl.v ? cl.c : p;
    return g[r * SIZE + c] === -1;
  };
  let lo = pos[i];
  while (lo > 0 && free(lo - 1)) lo--;
  let hi = pos[i];
  while (hi + cl.len < SIZE && free(hi + cl.len)) hi++;
  return [lo, hi];
}

function next(clouds: Cloud[], pos: Pos): Pos[] {
  const out: Pos[] = [];
  clouds.forEach((_, i) => {
    const [lo, hi] = room(clouds, pos, i);
    for (let p = lo; p <= hi; p++) {
      if (p === pos[i]) continue;
      const n = pos.slice();
      n[i] = p;
      out.push(n);
    }
  });
  return out;
}

const solved = (pos: Pos) => pos[0] === SIZE - 2;

/** The fewest moves from here to the sun, as the arrangements on the way; null when there is no way. */
export function solve(clouds: Cloud[], limit = 60_000): Cloud[][] | null {
  const start = clouds.map(along);
  const back = new Map<string, string | null>([[key(start), null]]);
  const states = new Map<string, Pos>([[key(start), start]]);
  const queue = [start];
  for (let k = 0; k < queue.length && k < limit; k++) {
    const pos = queue[k];
    if (solved(pos)) {
      const way: Cloud[][] = [];
      for (let at: string | null = key(pos); at !== null; at = back.get(at) ?? null) {
        const p = states.get(at)!;
        way.unshift(clouds.map((cl, i) => place(cl, p[i])));
      }
      return way;
    }
    for (const n of next(clouds, pos)) {
      const kn = key(n);
      if (back.has(kn)) continue;
      back.set(kn, key(pos));
      states.set(kn, n);
      queue.push(n);
    }
  }
  return null;
}

/** A random sky of clouds around Ciel, on his row; some will have no way out. */
function scatter(count: number, random: () => number): Cloud[] {
  const clouds: Cloud[] = [{ r: EXIT_ROW, c: Math.floor(random() * 2), len: 2, v: false }];
  const taken = grid(clouds, clouds.map(along));
  for (let tries = 0; clouds.length < count + 1 && tries < 200; tries++) {
    const v = random() < 0.55;
    const len = random() < 0.75 ? 2 : 3;
    const r = Math.floor(random() * (v ? SIZE - len + 1 : SIZE));
    const c = Math.floor(random() * (v ? SIZE : SIZE - len + 1));
    // Nothing else lies along Ciel's row: it would never let him by.
    if (!v && r === EXIT_ROW) continue;
    const cells = Array.from({ length: len }, (_, k) => (v ? (r + k) * SIZE + c : r * SIZE + c + k));
    if (cells.some((i) => taken[i] !== -1)) continue;
    for (const i of cells) taken[i] = clouds.length;
    clouds.push({ r, c, len, v });
  }
  return clouds;
}

/** The fewest moves to the sun from every arrangement this sky can reach. */
function distances(clouds: Cloud[], limit: number): { pos: Pos; d: number }[] | null {
  const seen = new Map<string, Pos>();
  const queue = [clouds.map(along)];
  seen.set(key(queue[0]), queue[0]);
  for (let k = 0; k < queue.length; k++) {
    if (queue.length > limit) return null;
    for (const n of next(clouds, queue[k])) {
      const kn = key(n);
      if (seen.has(kn)) continue;
      seen.set(kn, n);
      queue.push(n);
    }
  }
  // Moves go both ways: out from every solved arrangement at once.
  const d = new Map<string, number>();
  const wave = queue.filter(solved);
  for (const p of wave) d.set(key(p), 0);
  for (let k = 0; k < wave.length; k++) {
    const here = d.get(key(wave[k]))!;
    for (const n of next(clouds, wave[k])) {
      const kn = key(n);
      if (d.has(kn)) continue;
      d.set(kn, here + 1);
      wave.push(n);
    }
  }
  return wave.map((pos) => ({ pos, d: d.get(key(pos))! }));
}

/** The level's wish: a few moves at first, harder each level, then about 20. */
export const target = (level: number) => Math.min(22, 3 + Math.round(level * 1.4));

export function puzzle(level: number, random = Math.random, budgetMs = 900): Puzzle {
  const want = target(level);
  const until = Date.now() + budgetMs;
  let best: Puzzle | null = null;
  while (!best || (best.moves < want && Date.now() < until)) {
    const sky = scatter(5 + Math.floor(random() * 6) + Math.min(4, level >> 2), random);
    const all = distances(sky, 40_000);
    if (!all) continue;
    // Not already at the sun, nor a sky where Ciel just floats out.
    const fair = all.filter((s) => s.d >= 2);
    if (!fair.length) continue;
    // The nearest to the wish, a harder one before an easier one.
    const off = (s: { d: number }) => Math.abs(s.d - want) - (s.d >= want ? 0.5 : 0);
    const pick = fair.reduce((a, b) => (off(b) < off(a) ? b : a));
    if (!best || Math.abs(pick.d - want) < Math.abs(best.moves - want)) {
      best = { clouds: sky.map((cl, i) => place(cl, pick.pos[i])), moves: pick.d };
    }
  }
  return best;
}
