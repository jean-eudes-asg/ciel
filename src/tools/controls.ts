// Tools → Controls: what a desktop's quick settings hold, and more. On top,
// the battery and the buttons (screenshot, settings, lock, the power menu);
// then the sound with its speakers to choose from, the screens' brightness;
// the tiles (network, Wi-Fi, Bluetooth, power mode, night light, dark style,
// do not disturb, VPN, every microphone at once, keeping the computer awake,
// Ciel's eyes wide open meanwhile); and the user's own apps, from GNOME's dock
// or the taskbar. A tile with an arrow opens its list in place. Read when the
// panel comes on screen (src-tauri/src/controls.rs, quick.rs); nothing polls.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import type { Controls, PowerMode } from "../core/tools";
import { SHORTCUTS, displayKeys, effective } from "../core/shortcuts";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import type { ViewActions } from "../views/views";
import { N_, t, tl, type Msg } from "../i18n/i18n";
import { Tools } from "./store";
import { dueText } from "./timers";
import { throttle, toolSlider } from "./ui";
import type { ToolPanel } from "./view";

interface Tile {
  el: HTMLElement;
  label: HTMLElement;
  sub: HTMLElement;
}

/** A tile; with `more`, an arrow on its right opens its list. */
function tile(icon: string, label: Msg, onClick: () => void, more?: () => void): Tile {
  const sub = h("span", { class: "ctl-sub" });
  const name = h("span", { class: "ctl-label", text: label });
  // A div: the arrow inside is a button of its own.
  const el = h("div", { class: "ctl-tile", role: "button", tabindex: "0", onclick: onClick, onkeydown: (e: Event) => {
    if ((e as KeyboardEvent).key === "Enter" || (e as KeyboardEvent).key === " ") {
      e.preventDefault();
      onClick();
    }
  } },
    h("span", { class: "ctl-icon" }, svg(icon, 14, { stroke: 2 })),
    h("span", { class: "ctl-text" }, name, sub),
  );
  if (more) {
    el.classList.add("split");
    el.append(h("button", { class: "ctl-more", title: tl("More"), onclick: (e: Event) => {
      e.stopPropagation();
      more();
    } }, svg(ICONS.chevronRight, 11, { stroke: 2.2 })));
  }
  return { el, label: name, sub };
}

const AWAKE_STEPS = [30, 60, 120, -1] as const;

/** English keys, shown with t(). */
const POWER_MODES: Record<PowerMode, { icon: string; label: string }> = {
  "power-saver": { icon: ICONS.leaf, label: N_("Power saver") },
  balanced: { icon: ICONS.gauge, label: N_("Balanced") },
  performance: { icon: ICONS.bolt, label: N_("Performance") },
};

/** The lists a tile opens in place of the tiles. */
type Menu = "output" | "network" | "powerMode" | "power" | null;

/** The power menu: the first click arms, the second does it. */
const POWER: { id: string; icon: string; label: string; sure: boolean }[] = [
  { id: "suspend", icon: ICONS.moon, label: N_("Suspend"), sure: false },
  { id: "restart", icon: ICONS.arrowClockwise, label: N_("Restart"), sure: true },
  { id: "powerOff", icon: ICONS.power, label: N_("Power off"), sure: true },
  { id: "logOut", icon: ICONS.arrowUpRight, label: N_("Log out"), sure: true },
];

export function buildControls(actions: ViewActions): ToolPanel {
  const apply = (c: Controls | null) => {
    if (c) {
      Tools.controls = c;
      State.notify();
    }
  };
  const set = (what: string, value: number) => void Bridge.controlSet(what, value).then(apply);
  const choose = (what: string, id: string) => void Bridge.controlChoose(what, id).then(apply);
  const system = (what: string) => {
    actions.blip();
    void Bridge.systemAction(what);
  };
  let menu: Menu = null;
  let armed: string | null = null;
  const open = (m: Menu) => {
    actions.blip();
    menu = menu === m ? null : m;
    armed = null;
    State.notify();
  };

  // ── The top: the battery, then the buttons ──
  const battery = h("span", { class: "ctl-battery" });
  const button = (icon: string, title: Msg, onClick: () => void, filled = false) =>
    h("button", { class: "ctl-round", title, onclick: onClick }, svg(icon, 14, filled ? {} : { stroke: 2 }));
  const powerButton = button(ICONS.power, tl("Power"), () => open("power"));
  const top = h("div", { class: "ctl-top" },
    battery, h("span", { class: "grow" }),
    button(ICONS.camera, tl("Screenshot"), () => system("screenshot")),
    button(ICONS.gear, tl("Settings"), () => system("settings"), true),
    button(ICONS.lock, tl("Lock"), () => system("lock")),
    powerButton,
  );

  // ── Sound and brightness ──
  const volumeIcon = h("button", { class: "ctl-knob", title: tl("Mute"), onclick: () => {
    actions.blip();
    set("muted", Tools.controls?.muted ? 0 : 1);
  } });
  const volume = toolSlider(throttle(60, (v: number) => set("volume", v)));
  const volumeValue = h("span", { class: "ctl-value" });
  const outputButton = h("button", { class: "ctl-knob", title: tl("Sound output"), onclick: () => open("output") },
    svg(ICONS.chevronRight, 11, { stroke: 2.2 }));
  const volumeRow = h("div", { class: "ctl-slider-row" }, volumeIcon, volume.el, volumeValue, outputButton);
  // Brightness: DDC/CI is slow, so fewer steps while dragging.
  const brightIcon = h("span", { class: "ctl-knob static" }, svg(ICONS.sun, 13, { stroke: 2 }));
  const brightness = toolSlider(throttle(300, (v: number) => set("brightness", v)), (v: number) => set("brightness", v));
  const brightValue = h("span", { class: "ctl-value" });
  const brightRow = h("div", { class: "ctl-slider-row" }, brightIcon, brightness.el, brightValue, h("span", { class: "ctl-knob static" }));

  // ── The tiles ──
  const network = tile(ICONS.network, tl("Network"), () => openNetwork(), () => openNetwork());
  // Wi-Fi by its network's name, with the list; the network tile is then for the rest.
  const wifi = tile(ICONS.wifi, tl("Wi-Fi"), () => {
    actions.blip();
    set("wifi", Tools.controls?.wifi ? 0 : 1);
  }, () => openNetwork());
  const bt = tile(ICONS.bluetooth, tl("Bluetooth"), () => {
    actions.blip();
    set("bluetooth", Tools.controls?.bluetooth ? 0 : 1);
  });
  const powerMode = tile(ICONS.gauge, tl("Power mode"), () => open("powerMode"), () => open("powerMode"));
  const night = tile(ICONS.moon, tl("Night light"), () => {
    actions.blip();
    set("nightLight", Tools.controls?.nightLight ? 0 : 1);
  });
  const dark = tile(ICONS.contrast, tl("Dark style"), () => {
    actions.blip();
    set("dark", Tools.controls?.dark ? 0 : 1);
  });
  const dnd = tile(ICONS.bellOff, tl("Do not disturb"), () => {
    actions.blip();
    set("dnd", Tools.controls?.dnd ? 0 : 1);
  });
  dnd.el.classList.add("dnd");
  // One VPN: the tile turns it on or off; several: the list.
  const vpns = () => (Tools.controls?.connections ?? []).filter((c) => c.kind === "vpn");
  const vpn = tile(ICONS.shield, tl("VPN"), () => {
    const list = vpns();
    if (list.length === 1) {
      actions.blip();
      choose("connection", list[0].id);
    } else {
      open("network");
    }
  });
  const mic = tile(ICONS.mic, tl("Microphones"), () => {
    actions.blip();
    void Bridge.micToggle().then(() => Tools.refreshControls());
  });
  mic.el.classList.add("mic");
  const awake = tile(ICONS.coffee, tl("Keep awake"), () => {
    actions.blip();
    // Off → 30 min → 1 h → 2 h → until turned off → off.
    const c = Tools.controls;
    let step = -1;
    if (c?.awake) {
      const left = c.awakeUntil == null ? Infinity : (c.awakeUntil - Date.now()) / 60_000;
      step = left === Infinity ? 3 : left > 61 ? 2 : left > 31 ? 1 : 0;
    }
    const next = step + 1;
    void Bridge.awakeSet(next >= AWAKE_STEPS.length ? 0 : AWAKE_STEPS[next]).then(apply);
  });
  awake.el.classList.add("awake");

  function openNetwork() {
    // Linux lists its connections here; Windows has its own list beside the taskbar.
    if ((Tools.controls?.connections ?? []).length) open("network");
    else system("networkSettings");
  }

  const tiles = h("div", { class: "ctl-tiles" },
    network.el, wifi.el, bt.el, powerMode.el, night.el, dark.el, dnd.el, vpn.el, mic.el, awake.el);

  // ── The user's own apps ──
  const apps = h("div", { class: "ctl-apps" });
  let appsKey = "";

  // ── A list, in place of the tiles ──
  const listTitle = h("span", { class: "ctl-list-title" });
  const listRows = h("div", { class: "ctl-list-rows" });
  const listFoot = h("div", { class: "ctl-list-foot" });
  const list = h("div", { class: "ctl-list" },
    h("div", { class: "ctl-list-head" },
      h("button", { class: "ctl-round small", title: tl("Back"), onclick: () => open(null) }, svg(ICONS.chevronLeft, 12, { stroke: 2.2 })),
      listTitle,
    ),
    listRows,
    listFoot,
  );
  const main = h("div", { class: "ctl-main" }, h("div", { class: "ctl-sliders" }, volumeRow, brightRow), tiles, apps);
  const el = h("div", { class: "tool tool-controls" }, top, main, list);

  function row(icon: string, label: string, sub: string, on: boolean, onClick: () => void, cls = ""): HTMLElement {
    return h("button", { class: `ctl-row${on ? " on" : ""}${cls ? ` ${cls}` : ""}`, onclick: onClick },
      h("span", { class: "ctl-icon" }, svg(icon, 13, { stroke: 2 })),
      h("span", { class: "ctl-text" }, h("span", { class: "ctl-label", text: label }), ...(sub ? [h("span", { class: "ctl-sub", text: sub })] : [])),
      ...(on ? [h("span", { class: "ctl-check" }, svg(ICONS.check, 12, { stroke: 2.4 }))] : []),
    );
  }

  function settingsLink(label: Msg, what: string): HTMLElement {
    return h("button", { class: "tool-link", text: label, onclick: () => system(what) });
  }

  let listKey = "";
  function paintList(c: Controls | null) {
    const key = JSON.stringify([menu, armed, c?.outputs, c?.connections, c?.powerMode, c?.powerModes]);
    if (key === listKey) return;
    listKey = key;
    if (menu === "output") {
      listTitle.textContent = t("Sound output");
      listRows.replaceChildren(...(c?.outputs ?? []).map((d) =>
        row(ICONS.speakerOn, d.name, "", d.default, () => {
          actions.blip();
          choose("output", d.id);
        })));
      listFoot.replaceChildren(settingsLink(tl("Sound settings"), "soundSettings"));
    } else if (menu === "network") {
      listTitle.textContent = t("Network");
      const kinds: Record<string, [string, string]> = { wired: [ICONS.network, N_("Wired")], wifi: [ICONS.wifi, N_("Wi-Fi")], vpn: [ICONS.shield, N_("VPN")] };
      listRows.replaceChildren(...(c?.connections ?? []).map((x) => {
        const [icon, kind] = kinds[x.kind];
        return row(icon, x.name, x.active ? `${t(kind)} · ${t("Connected")}` : t(kind), x.active, () => {
          actions.blip();
          choose("connection", x.id);
        });
      }));
      listFoot.replaceChildren(settingsLink(tl("Network settings"), "networkSettings"));
    } else if (menu === "powerMode") {
      listTitle.textContent = t("Power mode");
      listRows.replaceChildren(...(c?.powerModes ?? []).map((m) =>
        row(POWER_MODES[m].icon, t(POWER_MODES[m].label), "", c?.powerMode === m, () => {
          actions.blip();
          choose("powerMode", m);
        })));
      listFoot.replaceChildren(settingsLink(tl("Power settings"), "powerSettings"));
    } else if (menu === "power") {
      listTitle.textContent = t("Power");
      listRows.replaceChildren(...POWER.map((p) => {
        const ready = armed === p.id;
        return row(p.icon, t(p.label), ready ? t("Click again to confirm") : "", false, () => {
          if (p.sure && !ready) {
            actions.blip();
            armed = p.id;
            State.notify();
            return;
          }
          armed = null;
          menu = null;
          system(p.id);
          State.notify();
        }, ready ? "armed" : "");
      }));
      listFoot.replaceChildren();
    }
  }

  function onOff(on: boolean | null): string {
    return on == null ? t("Not here") : on ? t("On") : t("Off");
  }

  function micKeys(): string {
    const def = SHORTCUTS.find((d) => d.id === "micToggle");
    if (!def) return "";
    const b = effective(def, State.settings.shortcuts);
    return b.enabled && b.keys ? displayKeys(b.keys) : "";
  }

  function paintApps() {
    const key = Tools.apps.map((a) => a.id).join("|");
    if (key === appsKey) return;
    appsKey = key;
    apps.style.display = Tools.apps.length ? "" : "none";
    apps.replaceChildren(...Tools.apps.map((a) =>
      h("button", { class: "ctl-app", title: a.name, onclick: () => {
        actions.blip();
        void Bridge.launcherOpen(a.id);
      } }, a.icon ? h("img", { src: a.icon, alt: "", draggable: "false" }) : h("span", { text: a.name.slice(0, 1).toUpperCase() }))));
  }

  return {
    el,
    shown() {
      menu = null;
      armed = null;
      void Tools.refreshControls();
      void Tools.refreshApps();
    },
    sync() {
      const c = Tools.controls;
      el.classList.toggle("listing", menu != null);
      powerButton.classList.toggle("on", menu === "power");
      paintList(c);
      paintApps();

      const b = c?.battery;
      battery.style.display = b ? "" : "none";
      if (b) {
        battery.textContent = `${Math.round(b.percent)}%`;
        battery.classList.toggle("low", b.percent <= 15 && !b.charging);
        battery.classList.toggle("charging", b.charging);
        battery.style.setProperty("--level", `${Math.max(0, Math.min(100, b.percent))}%`);
      }

      const pct = (v: number | null | undefined) => (v == null ? "" : `${Math.round(v * 100)}`);
      volume.set(c?.volume ?? null);
      volumeIcon.replaceChildren(svg(c?.muted || c?.volume === 0 ? ICONS.volume0 : (c?.volume ?? 0) < 0.34 ? ICONS.volume1 : (c?.volume ?? 0) < 0.67 ? ICONS.volume2 : ICONS.volume3, 14, { stroke: 2 }));
      volumeRow.classList.toggle("muted", !!c?.muted);
      volumeValue.textContent = c?.muted ? t("Muted") : pct(volume.active ? null : c?.volume);
      outputButton.style.visibility = (c?.outputs.length ?? 0) > 1 ? "" : "hidden";
      outputButton.classList.toggle("on", menu === "output");
      brightRow.style.display = c?.brightness == null && c ? "none" : "";
      brightness.set(c?.brightness ?? null);
      brightValue.textContent = pct(c?.brightness);

      const net = c?.network;
      network.el.style.display = net && (net.kind !== "wifi" || c?.wifi == null) ? "" : "none";
      if (net) {
        network.el.classList.toggle("on", !!net.kind);
        const kind = net.kind === "wired" ? t("Wired") : net.kind === "wifi" ? t("Wi-Fi") : net.kind === "mobile" ? t("Mobile") : "";
        network.label.textContent = net.kind === "wifi" && net.name ? net.name : kind || t("Network");
        network.sub.textContent = !net.kind ? t("Not connected") : net.kind === "wifi" ? t("Wi-Fi") : t("Connected");
        network.el.querySelector(".ctl-icon")!.replaceChildren(svg(net.kind === "wifi" ? ICONS.wifi : ICONS.network, 14, { stroke: 2 }));
      }

      // What this computer doesn't have isn't shown.
      wifi.el.style.display = c?.wifi == null ? "none" : "";
      wifi.el.classList.toggle("on", !!c?.wifi);
      const ssid = net?.kind === "wifi" ? net.name : "";
      wifi.label.textContent = ssid || t("Wi-Fi");
      wifi.sub.textContent = ssid ? t("Connected") : c ? onOff(c.wifi) : "";
      bt.el.style.display = c?.bluetooth == null ? "none" : "";
      bt.el.classList.toggle("on", !!c?.bluetooth);
      bt.sub.textContent = c ? onOff(c.bluetooth) : "";

      powerMode.el.style.display = c?.powerMode ? "" : "none";
      if (c?.powerMode) {
        const m = POWER_MODES[c.powerMode] ?? POWER_MODES.balanced;
        powerMode.el.classList.toggle("on", c.powerMode !== "balanced");
        // The mode as the title: "Power mode" alone would be cut short in a tile with an arrow.
        powerMode.label.textContent = t(m.label);
        powerMode.sub.textContent = t("Power mode");
        powerMode.el.querySelector(".ctl-icon")!.replaceChildren(svg(m.icon, 14, { stroke: 2 }));
      }

      night.el.style.display = c?.nightLight == null ? "none" : "";
      night.el.classList.toggle("on", !!c?.nightLight);
      night.sub.textContent = c ? onOff(c.nightLight) : "";
      dark.el.style.display = c?.dark == null ? "none" : "";
      dark.el.classList.toggle("on", !!c?.dark);
      dark.sub.textContent = c ? onOff(c.dark) : "";
      dnd.el.classList.toggle("on", !!c?.dnd);
      dnd.sub.textContent = c ? onOff(c.dnd) : "";

      const v = vpns();
      vpn.el.style.display = v.length ? "" : "none";
      const up = v.find((x) => x.active);
      vpn.el.classList.toggle("on", !!up);
      vpn.sub.textContent = up ? up.name : t("Off");

      const muted = !!c?.micMuted;
      mic.el.classList.toggle("on", muted);
      mic.el.querySelector(".ctl-icon")!.replaceChildren(svg(muted ? ICONS.micOff : ICONS.mic, 14, { stroke: 2 }));
      const keys = micKeys();
      mic.sub.textContent = c && c.mics === 0 ? t("None plugged in") : muted ? t("All muted") : keys ? t("On · {keys}", { keys }) : t("On");
      mic.el.title = keys ? t("Mute every microphone at once ({keys})", { keys }) : t("Mute every microphone at once");

      awake.el.classList.toggle("on", !!c?.awake);
      awake.sub.textContent = !c?.awake ? t("Off") : c.awakeUntil == null ? t("Until I say") : t("Until {time}", { time: dueText(c.awakeUntil) });
    },
  };
}
