// Seeded randomness and gradient noise for the skies: the same seed always
// draws the same clouds and stars, at any resolution.

/** A small, fast seeded PRNG (mulberry32): numbers in [0, 1). */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A well-mixed 32-bit hash of integers. */
export function hash(...values: number[]): number {
  let h = 0x811c9dc5;
  for (const v of values) {
    h = Math.imul(h ^ (v | 0), 0x01000193);
    h ^= h >>> 13;
    h = Math.imul(h, 0x5bd1e995);
    h ^= h >>> 15;
  }
  return h >>> 0;
}

/** hash() as a number in [0, 1). */
export const hash01 = (...values: number[]) => hash(...values) / 4294967296;

const fade = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);

/**
 * 2D gradient (Perlin) noise in about [-1, 1], optionally periodic in x
 * (`periodX` lattice cells) so a texture can scroll forever without a seam.
 */
export class Noise {
  private grads: Float32Array;
  private perm: Uint16Array;

  constructor(seed: number) {
    const r = rng(seed);
    this.grads = new Float32Array(512);
    for (let i = 0; i < 256; i++) {
      const a = r() * Math.PI * 2;
      this.grads[i * 2] = Math.cos(a);
      this.grads[i * 2 + 1] = Math.sin(a);
    }
    const p = Array.from({ length: 256 }, (_, i) => i);
    for (let i = 255; i > 0; i--) {
      const j = Math.floor(r() * (i + 1));
      [p[i], p[j]] = [p[j], p[i]];
    }
    this.perm = new Uint16Array(512);
    for (let i = 0; i < 512; i++) this.perm[i] = p[i & 255];
  }

  private dot(ix: number, iy: number, dx: number, dy: number): number {
    const g = this.perm[(this.perm[ix & 255] + iy) & 511] & 255;
    return this.grads[g * 2] * dx + this.grads[g * 2 + 1] * dy;
  }

  noise(x: number, y: number, periodX = 0): number {
    const x0 = Math.floor(x);
    const y0 = Math.floor(y);
    const fx = x - x0;
    const fy = y - y0;
    let ix0 = x0;
    let ix1 = x0 + 1;
    if (periodX > 0) {
      ix0 = ((ix0 % periodX) + periodX) % periodX;
      ix1 = ((ix1 % periodX) + periodX) % periodX;
    }
    const iy0 = y0 & 255;
    const iy1 = (y0 + 1) & 255;
    const u = fade(fx);
    const v = fade(fy);
    const a = this.dot(ix0, iy0, fx, fy);
    const b = this.dot(ix1, iy0, fx - 1, fy);
    const c = this.dot(ix0, iy1, fx, fy - 1);
    const d = this.dot(ix1, iy1, fx - 1, fy - 1);
    return (a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v) * 1.41;
  }

  /** Fractal sum of `octaves` layers, in about [-1, 1]. */
  fbm(x: number, y: number, octaves = 5, periodX = 0, gain = 0.5): number {
    let sum = 0;
    let amp = 1;
    let norm = 0;
    let f = 1;
    for (let o = 0; o < octaves; o++) {
      sum += amp * this.noise(x * f, y * f + o * 17.3, periodX ? periodX * f : 0);
      norm += amp;
      amp *= gain;
      f *= 2;
    }
    return sum / norm;
  }
}

/** Hermite step between two edges. */
export function smoothstep(e0: number, e1: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
}

export const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);
