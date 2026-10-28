import { defineConfig, type Plugin } from "vite";
import { existsSync, mkdirSync, readdirSync, copyFileSync, createReadStream } from "node:fs";
import { resolve, join, extname } from "node:path";

// ───────────────────────────────────────────────────────────────────────────────
// THE one and only place the sound folder is declared.
// Ciel's 29 WAVs live in this repo, in sounds/. They are synthesized from code by
// scripts/gen-sounds.mjs: `npm run sounds` makes them again, byte for byte.
export const SOUNDS_DIR = resolve(__dirname, "sounds");
// ───────────────────────────────────────────────────────────────────────────────

/**
 * Serves SOUNDS_DIR at /sounds/*.wav in dev, and copies it into dist/sounds on build,
 * so the WAVs ship inside the installer without living under public/.
 */
function sharedSounds(): Plugin {
  const prefix = "/sounds/";
  return {
    name: "ciel-shared-sounds",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith(prefix)) return next();
        const name = decodeURIComponent(req.url.slice(prefix.length).split("?")[0]);
        if (name.includes("/") || name.includes("\\") || extname(name) !== ".wav") return next();
        const file = join(SOUNDS_DIR, name);
        if (!existsSync(file)) return next();
        res.setHeader("Content-Type", "audio/wav");
        createReadStream(file).pipe(res);
      });
    },
    closeBundle() {
      const out = resolve(__dirname, "dist/sounds");
      if (!existsSync(SOUNDS_DIR)) {
        this.warn(`sounds not found at ${SOUNDS_DIR} (run \`npm run sounds\`) — the build will ship without audio`);
        return;
      }
      mkdirSync(out, { recursive: true });
      for (const f of readdirSync(SOUNDS_DIR)) {
        if (extname(f) === ".wav") copyFileSync(join(SOUNDS_DIR, f), join(out, f));
      }
    },
  };
}

export default defineConfig({
  plugins: [sharedSounds()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    // Cargo writes and locks files under target/ while Vite starts; watching them
    // crashes Vite on Windows with EBUSY. Tauri watches src-tauri/ itself.
    watch: { ignored: ["**/target/**", "**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
    emptyOutDir: true,
    rollupOptions: {
      input: {
        island: resolve(__dirname, "index.html"),
        settings: resolve(__dirname, "settings.html"),
        cloud: resolve(__dirname, "cloud.html"),
        pill: resolve(__dirname, "pill.html"),
        wallpaper: resolve(__dirname, "wallpaper.html"),
        screensaver: resolve(__dirname, "screensaver.html"),
        games: resolve(__dirname, "games.html"),
        // The island in demo mode in a plain browser page (src/demo/web.ts).
        demo: resolve(__dirname, "demo.html"),
      },
    },
  },
});
