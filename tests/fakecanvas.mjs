// Just enough of Canvas 2D for the skies (src/sky) to draw in Node: every
// call is accepted and recorded, gradients and patterns are plain objects,
// ImageData holds real bytes, and an OffscreenCanvas encodes to "PNG" bytes
// that only say how big it was.

class FakeImageData {
  constructor(width, height) {
    this.width = width;
    this.height = height;
    this.data = new Uint8ClampedArray(width * height * 4);
  }
}

class FakePath2D {
  constructor() {
    this.ops = 0;
  }
}
for (const name of ["moveTo", "lineTo", "arc", "ellipse", "closePath", "rect", "bezierCurveTo", "quadraticCurveTo", "arcTo"]) {
  FakePath2D.prototype[name] = function () {
    this.ops++;
  };
}

const gradient = () => ({ stops: [], addColorStop(offset, color) { this.stops.push([offset, color]); } });

/** A 2D context recording every call as [name, ...args]. */
function fakeContext(canvas) {
  const calls = [];
  const state = {
    canvas,
    calls,
    createLinearGradient: (...a) => (calls.push(["createLinearGradient", ...a]), gradient()),
    createRadialGradient: (...a) => (calls.push(["createRadialGradient", ...a]), gradient()),
    createPattern: () => ({}),
    createImageData: (w, h) => new FakeImageData(w, h),
    getImageData: (x, y, w, h) => new FakeImageData(w, h),
    measureText: (text) => ({ width: String(text).length * 6 }),
  };
  return new Proxy(state, {
    get(target, prop) {
      if (prop in target) return target[prop];
      if (typeof prop === "symbol") return undefined;
      return (...args) => {
        calls.push([prop, ...args.map((a) => (a && typeof a === "object" && "width" in a && "height" in a ? `canvas ${a.width}x${a.height}` : a))]);
      };
    },
    set(target, prop, value) {
      target[prop] = value;
      return true;
    },
  });
}

export class FakeCanvas {
  constructor(width = 300, height = 150) {
    this.width = width;
    this.height = height;
    this.ctx = null;
  }
  getContext() {
    this.ctx ??= fakeContext(this);
    return this.ctx;
  }
  async convertToBlob() {
    const bytes = new TextEncoder().encode(`PNG ${this.width}x${this.height}`);
    return { arrayBuffer: async () => bytes.buffer };
  }
}

/** Installs ImageData, Path2D and OffscreenCanvas on the global object. */
export function installFakeCanvas() {
  globalThis.ImageData = FakeImageData;
  globalThis.Path2D = FakePath2D;
  globalThis.OffscreenCanvas = FakeCanvas;
}
