// Settings → Integrations → Notifications: Ciel for Chrome. The bridge is made
// known to the browsers only from here, after the user has seen every place
// written (src-tauri/src/browser_bridge.rs); the extension itself is loaded by
// hand from the release's Ciel-for-Chrome.zip. Gmail is a pill of its own. On
// Linux, the other apps' notifications are a switch (src-tauri/src/notify.rs).

import { Bridge } from "../core/bridge";
import { SHOW_CHOICES } from "../core/notifications";
import { toggle } from "../hub/ui";
import { t } from "../i18n/i18n";
import { clear, h } from "../views/dom";
import { statusDot } from "./parts";

const RELEASES = "https://github.com/jean-eudes-asg/ciel/releases/latest";

export interface NotificationPrefs {
  /** How long a notification shows, in seconds; 0 until opened or dismissed. */
  seconds: number;
  setSeconds(seconds: number): void;
  desktop: boolean;
  setDesktop(on: boolean): void;
}

const LINUX = !navigator.userAgent.includes("Windows");

/** How long a notification shows in the island. */
function durationRow(prefs: NotificationPrefs): HTMLElement {
  const select = h("select", {}) as HTMLSelectElement;
  for (const s of SHOW_CHOICES) {
    const label = s === 0 ? t("Until dismissed") : t("{n} seconds", { n: s });
    select.append(h("option", { value: String(s), text: label }));
  }
  select.value = String(SHOW_CHOICES.includes(prefs.seconds as never) ? prefs.seconds : 7);
  select.addEventListener("change", () => prefs.setSeconds(Number(select.value)));
  return h("div", { class: "row" }, h("label", { text: t("Show for") }), select);
}

export function notificationsSection(prefs: NotificationPrefs): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column;gap:12px" });
  const section = h("section", {}, h("h2", {}, h("span", { text: t("Notifications") })), body);

  async function draw() {
    const status = await Bridge.browserBridgeStatus();
    const installed = status?.installed === true;
    const gnome = LINUX ? await gnomeRows() : [];
    clear(body);
    body.append(durationRow(prefs));
    if (LINUX) {
      body.append(
        h("div", { class: "row" }, h("label", { text: t("Desktop apps") }), toggle(prefs.desktop, prefs.setDesktop)),
        h("div", {
          class: "hint",
          text: t("Your other apps' notifications (Thunderbird, Slack, system messages...) show in the island. On GNOME, its own banners are off while this is on, and come back after."),
        }),
        ...gnome,
      );
    }
    body.append(
      h("div", {
        class: "hint",
        text: t("Websites' notifications from Chrome, Edge or Brave show in the island, through the Ciel for Chrome extension, instead of the browser's popups."),
      }),
      h("div", { class: "row" },
        h("label", { text: t("Browsers") }),
        statusDot(installed),
        h("span", { class: "hint", text: installed ? t("installed") : t("not installed") }),
        installed
          ? h("button", { class: "danger", text: t("Disconnect"), onclick: () => review(false, status?.places ?? []) })
          : h("button", { class: "primary", text: t("Connect"), onclick: () => review(true, status?.places ?? []) }),
      ),
      h("div", {
        class: "hint",
        text: t("Then add the extension: unzip Ciel-for-Chrome.zip from the latest release, open chrome://extensions, turn on Developer mode and choose Load unpacked."),
      }),
      h("div", { class: "row" },
        h("button", { text: t("Get the extension"), onclick: () => void Bridge.openUrl(RELEASES) }),
      ),
    );
  }

  /** GNOME: Ciel's extension, so Open goes to the very chat or page. */
  async function gnomeRows(): Promise<HTMLElement[]> {
    const ext = await Bridge.gnomeExtensionStatus();
    if (!ext?.available) return [];
    const state = ext.active ? t("installed") : ext.installed ? t("Restart GNOME Shell to turn it on: Alt+F2, then r (or log out and back in).") : t("not installed");
    return [
      h("div", { class: "row" },
        h("label", { text: t("Open in the app") }),
        statusDot(ext.active),
        h("span", { class: "hint", text: state }),
        ext.installed
          ? h("button", { class: "danger", text: t("Remove"), onclick: () => review(false, ext.places, (on) => Bridge.gnomeExtensionSet(on), [t("Install"), t("Remove")]) })
          : h("button", { class: "primary", text: t("Install"), onclick: () => review(true, ext.places, (on) => Bridge.gnomeExtensionSet(on), [t("Install"), t("Remove")]) }),
      ),
      h("div", {
        class: "hint",
        text: t("On GNOME, Open goes to the very chat or page, as GNOME's own notifications do, through a small Ciel extension for GNOME Shell. Without it, Open brings the app's window forward."),
      }),
    ];
  }

  /** Every place, before anything is written or removed. */
  function review(
    on: boolean,
    places: string[],
    apply: (on: boolean) => Promise<unknown> = (o) => Bridge.browserBridgeSet(o),
    [yes, no] = [t("Connect"), t("Disconnect")],
  ) {
    clear(body);
    const list = h("div", { class: "path", style: "display:flex;flex-direction:column;gap:4px;white-space:pre-wrap" });
    for (const place of places) list.append(h("span", { text: place }));
    body.append(
      h("div", { class: "hint", text: on ? t("Ciel writes these, and nothing else:") : t("Ciel removes these:") }),
      list,
      h("div", { class: "row" },
        h("button", { text: t("Back"), onclick: () => void draw() }),
        h("button", {
          class: on ? "primary" : "danger",
          text: on ? yes : no,
          onclick: async () => {
            try {
              await apply(on);
            } catch (e) {
              console.warn("notifications: could not apply", e);
            }
            await draw();
          },
        }),
      ),
    );
  }

  void draw();
  return section;
}
