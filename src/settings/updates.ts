// Settings → General → Updates: Ciel's own (src-tauri/src/updates.rs). "Check
// now" asks the latest release; a switch makes it once a day, off by default
// (the network only reaches what the user turned on). A newer version installs
// only on "Install and restart"; a .deb or .rpm, which can't replace itself,
// gets the release page.

import { Bridge, onEvent, type UpdateStatus } from "../core/bridge";
import { toggle } from "../hub/ui";
import { t } from "../i18n/i18n";
import { clear, h } from "../views/dom";

const RELEASES = "https://github.com/jean-eudes-asg/ciel/releases/latest";

export interface UpdatePrefs {
  /** Once a day, on its own. */
  auto: boolean;
  setAuto(on: boolean): void;
}

/** What the last check or install says, under the buttons. */
export function updateLine(s: UpdateStatus): string {
  if (!s.ready) return t("This build can't update itself: new versions are on the website.");
  if (s.busy && s.progress != null) return t("Downloading… {percent}%", { percent: Math.round(s.progress * 100) });
  if (s.busy) return t("Checking…");
  if (s.error) return t("Something went wrong: {error}", { error: s.error });
  if (s.available) return t("Ciel {version} is out.", { version: s.available });
  if (s.checkedAt > 0) return t("Ciel is up to date.");
  return "";
}

export function updatesSection(prefs: UpdatePrefs): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column;gap:12px" });
  const section = h("section", {}, h("h2", {}, h("span", { text: t("Updates") })), body);
  let status: UpdateStatus | null = null;

  const current = h("span");
  const check = h("button", { text: t("Check now") }) as HTMLButtonElement;
  check.addEventListener("click", () => {
    void Bridge.updateCheck().then((s) => {
      if (s) show(s);
    });
  });
  const line = h("div", { class: "hint" });
  // A newer version: installed on a press (Windows, an AppImage), or downloaded by hand.
  const install = h("button", { class: "primary" }) as HTMLButtonElement;
  install.addEventListener("click", () => {
    if (status?.canInstall) void Bridge.updateInstall();
    else void Bridge.openUrl(RELEASES);
  });
  const notes = h("button", { text: t("What's new") });
  notes.addEventListener("click", () => void Bridge.openUrl(RELEASES));
  const offer = h("div", { class: "row" }, install, notes);

  function show(s: UpdateStatus) {
    status = s;
    current.textContent = s.current;
    check.disabled = s.busy || !s.ready;
    line.textContent = updateLine(s);
    line.style.display = line.textContent ? "" : "none";
    offer.style.display = s.available && s.ready ? "" : "none";
    install.textContent = s.canInstall ? t("Install and restart") : t("Download");
    install.disabled = s.busy;
  }

  clear(body);
  body.append(
    h("div", { class: "row" }, h("label", { text: t("Version") }), current, check),
    line,
    offer,
    h("div", { class: "row" },
      h("label", { text: t("Check for updates automatically") }),
      toggle(prefs.auto, prefs.setAuto),
    ),
    h("div", {
      class: "hint",
      text: t("Once a day, Ciel asks its latest release on GitHub whether a newer version is out. Nothing else is sent, and nothing installs without your click."),
    }),
  );
  void onEvent<UpdateStatus>("update-status", show);
  void Bridge.updateStatus().then((s) => {
    if (s) show(s);
  });
  return section;
}
