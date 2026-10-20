// latest.json for Ciel's updater (src-tauri/src/updates.rs), written by the
// release workflow from the signatures the build left next to the packages
// (scripts/pack.mjs): the version, its notes, and for each platform the package
// and its signature. Windows gets the installer (an MSI install, the MSI), Linux
// the AppImage — a .deb or .rpm is only told, and sent to the release page.
//
// usage: node scripts/latest-json.mjs <version> <release dir> [notes file] > latest.json

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** Each package the updater installs, and the platform keys that pick it. */
const PACKAGES = [
  { keys: ["windows-x86_64", "windows-x86_64-nsis"], file: (v) => `Ciel-Windows-${v}-setup.exe` },
  { keys: ["windows-x86_64-msi"], file: (v) => `Ciel-Windows-${v}.msi` },
  { keys: ["linux-x86_64-appimage"], file: (v) => `Ciel-Linux-${v}-x86_64.AppImage` },
];

/**
 * `sigs`: signature files by name ("Ciel-Windows-0.2.7-setup.exe.sig" → its
 * text). A package without its signature is left out: the updater would
 * refuse it anyway.
 */
export function latestJson({ version, repo, notes, date, sigs }) {
  const platforms = {};
  for (const { keys, file } of PACKAGES) {
    const name = file(version);
    const signature = sigs[`${name}.sig`]?.trim();
    if (!signature) continue;
    const url = `https://github.com/${repo}/releases/download/v${version}/${name}`;
    for (const key of keys) platforms[key] = { signature, url };
  }
  return { version, notes, pub_date: date, platforms };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const [version, dir, notesFile] = process.argv.slice(2);
  if (!version || !dir) {
    console.error("usage: node scripts/latest-json.mjs <version> <release dir> [notes file]");
    process.exit(2);
  }
  const sigs = {};
  for (const name of readdirSync(dir).filter((f) => f.endsWith(".sig"))) {
    sigs[name] = readFileSync(join(dir, name), "utf8");
  }
  const json = latestJson({
    version,
    repo: process.env.GITHUB_REPOSITORY || "jean-eudes-asg/ciel",
    notes: notesFile ? readFileSync(notesFile, "utf8").trim() : "",
    date: new Date().toISOString(),
    sigs,
  });
  if (Object.keys(json.platforms).length === 0) {
    console.error("No signed package: no latest.json.");
    process.exit(1);
  }
  process.stdout.write(`${JSON.stringify(json, null, 2)}\n`);
}
