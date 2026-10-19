// The website (site/), published on GitHub Pages by .github/workflows/pages.yml:
// the real sky renderer (src/sky) and the real Ciel (src/cloud) in any browser.
//
//   npm run site:dev     → http://localhost:5180/site/
//   npm run site:build   → site-dist/ (index.html at its root, relative links)
//
// demo.html (the island in demo mode) is built alongside when it exists, so the
// page's "Try it" frame can load it, with Ciel's sounds next to it (sounds/,
// for the demo's sound toggle). Nothing here touches the app's own build
// (vite.config.ts), and no Tauri.

import { defineConfig, type Plugin } from "vite";
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const OUT = resolve(__dirname, "site-dist");
const PAGE = resolve(__dirname, "site/index.html");
const DEMO = resolve(__dirname, "demo.html");
const SOUNDS = resolve(__dirname, "sounds");

/** The demo's sound toggle fetches sounds/<name>.wav next to demo.html. */
function demoSounds(): Plugin {
  return {
    name: "ciel-site-sounds",
    apply: "build",
    closeBundle() {
      if (!existsSync(DEMO) || !existsSync(SOUNDS)) return;
      const out = resolve(OUT, "sounds");
      mkdirSync(out, { recursive: true });
      for (const f of readdirSync(SOUNDS)) if (f.endsWith(".wav")) copyFileSync(resolve(SOUNDS, f), resolve(out, f));
    },
  };
}

/**
 * The page is site/index.html, built from the repository root so that
 * demo.html and the sources it imports resolve exactly as in the app. Vite
 * writes it to site-dist/site/index.html; Pages wants it at the root, so it
 * moves up a level and its relative links lose one "../".
 */
function pageAtRoot(): Plugin {
  return {
    name: "ciel-site-page-at-root",
    apply: "build",
    closeBundle() {
      const nested = resolve(OUT, "site/index.html");
      if (!existsSync(nested)) return;
      const html = readFileSync(nested, "utf8").replace(/(\s(?:src|href|content)=["'])\.\.\//g, "$1./");
      writeFileSync(resolve(OUT, "index.html"), html);
      rmSync(resolve(OUT, "site"), { recursive: true, force: true });
    },
  };
}

/** In dev, the root of the server is the repository: send "/" to the page. */
function devLanding(): Plugin {
  return {
    name: "ciel-site-dev-landing",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url !== "/" && req.url !== "/index.html") return next();
        res.statusCode = 302;
        res.setHeader("Location", "/site/");
        res.end();
      });
    },
  };
}

export default defineConfig({
  root: __dirname,
  base: "./",
  // og.jpg lives at a fixed URL for social cards: site/public is copied as is.
  publicDir: resolve(__dirname, "site/public"),
  // A missing page is a 404 (the "Try it" frame checks for demo.html), not index.html.
  appType: "mpa",
  plugins: [pageAtRoot(), devLanding(), demoSounds()],
  clearScreen: false,
  server: {
    port: 5180,
    host: "127.0.0.1",
    open: "/site/",
    // Same as the app: cargo's files under target/ crash Vite's watcher on Windows.
    watch: { ignored: ["**/target/**", "**/src-tauri/**", "**/site-dist/**"] },
  },
  preview: { port: 5181, host: "127.0.0.1" },
  build: {
    outDir: OUT,
    emptyOutDir: true,
    target: ["es2020", "chrome100", "edge100", "firefox100", "safari15"],
    sourcemap: false,
    // Files, not data: URLs, so the page stays small and pictures are cached.
    assetsInlineLimit: 0,
    rollupOptions: {
      input: {
        site: PAGE,
        ...(existsSync(DEMO) ? { demo: DEMO } : {}),
      },
    },
  },
});
