// Ciel for Chrome — the extension's worker. It holds the line to Ciel (its
// bridge, `ciel-hook`, started by the browser through native messaging),
// hands on what pages show, and when the island's Open is clicked brings the
// tab forward and passes the click to the page. Without Ciel, or without its
// bridge installed (Ciel → Settings → Notifications), it does nothing and the
// pages notify as usual.

const HOST = "io.github.jeaneudesasg.ciel";
/** Icons are made this small (px) before they go to Ciel. */
const ICON = 64;
/** Notifications whose way back is kept, newest last. */
const KEPT = 100;

/** Sites by their usual name. */
const SITES = {
  "mail.google.com": "Gmail",
  "calendar.google.com": "Google Calendar",
  "chat.google.com": "Google Chat",
  "web.whatsapp.com": "WhatsApp",
  "app.slack.com": "Slack",
  "discord.com": "Discord",
  "teams.microsoft.com": "Microsoft Teams",
  "teams.live.com": "Microsoft Teams",
  "outlook.live.com": "Outlook",
  "outlook.office.com": "Outlook",
  "web.telegram.org": "Telegram",
  "www.messenger.com": "Messenger",
  "www.linkedin.com": "LinkedIn",
  "github.com": "GitHub",
};

let port = null;
let ciel = false;
/** Where each notification came from: its tab, and the page's own key. */
const origins = new Map();

function siteName(url) {
  try {
    const host = new URL(url).hostname;
    return SITES[host] ?? host.replace(/^www\./, "");
  } catch {
    return "Chrome";
  }
}

async function tellTabs(message) {
  for (const tab of await chrome.tabs.query({})) {
    if (tab.id != null) chrome.tabs.sendMessage(tab.id, message).catch(() => {});
  }
}

function setCiel(on) {
  if (ciel === on) return;
  ciel = on;
  void tellTabs({ type: "status", on });
}

/** Opens the line to Ciel's bridge, if it isn't open. */
function connect() {
  if (port) return;
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch {
    port = null;
    return;
  }
  port.onMessage.addListener(fromCiel);
  port.onDisconnect.addListener(() => {
    // No bridge installed, or the browser ended it: the pages notify as usual.
    void chrome.runtime.lastError;
    port = null;
    setCiel(false);
  });
}

async function fromCiel(message) {
  if (message?.type === "status") {
    setCiel(message.ciel === true);
    return;
  }
  const origin = origins.get(message?.key);
  if (!origin) return;
  if (message.type === "click") {
    // Open: the tab comes forward, and the page gets the click it would have had.
    await chrome.tabs.update(origin.tabId, { active: true }).catch(() => {});
    await chrome.windows.update(origin.windowId, { focused: true }).catch(() => {});
    chrome.tabs.sendMessage(origin.tabId, { type: "click", key: origin.pageKey }).catch(() => {});
  } else if (message.type === "close") {
    origins.delete(message.key);
    chrome.tabs.sendMessage(origin.tabId, { type: "close", key: origin.pageKey }).catch(() => {});
  }
}

function base64(buffer) {
  const bytes = new Uint8Array(buffer);
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

/** The icon as a small PNG data URL, or null. */
async function iconData(url) {
  if (!url || !/^https?:|^data:image\//.test(url)) return null;
  try {
    const blob = await (await fetch(url, { credentials: "omit" })).blob();
    const image = await createImageBitmap(blob);
    const scale = ICON / Math.max(image.width, image.height, 1);
    const w = Math.max(1, Math.round(image.width * scale));
    const h = Math.max(1, Math.round(image.height * scale));
    const canvas = new OffscreenCanvas(w, h);
    canvas.getContext("2d").drawImage(image, 0, 0, w, h);
    const png = await canvas.convertToBlob({ type: "image/png" });
    return `data:image/png;base64,${base64(await png.arrayBuffer())}`;
  } catch {
    return null;
  }
}

async function forward(message, tab) {
  connect();
  if (!port || !ciel) return;
  const key = `${tab.id}|${message.key}`;
  origins.set(key, { tabId: tab.id, windowId: tab.windowId, pageKey: message.key });
  while (origins.size > KEPT) origins.delete(origins.keys().next().value);
  port.postMessage({
    type: "notification",
    key,
    app: siteName(tab.url ?? ""),
    title: String(message.title ?? "").slice(0, 300),
    body: String(message.body ?? "").slice(0, 2000),
    icon: (await iconData(message.icon)) ?? (await iconData(tab.favIconUrl)),
    tag: String(message.tag ?? "").slice(0, 200),
  });
}

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (message?.type === "hello") {
    connect();
    reply({ ciel });
    return;
  }
  if (message?.type === "notification" && sender.tab?.id != null) void forward(message, sender.tab);
});

connect();
