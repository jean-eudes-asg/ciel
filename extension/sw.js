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
    // Ciel (back) on the line: the players tell it what plays.
    for (const tabId of mediaTabs) chrome.tabs.sendMessage(tabId, { type: "media-control", action: "report" }).catch(() => {});
    return;
  }
  if (message?.type === "media-control") {
    const tabId = Number(message.key);
    if (!mediaTabs.has(tabId)) return;
    if (message.action === "focus") {
      // The cover, or "Open YouTube Music": the tab comes forward.
      const tab = await chrome.tabs.update(tabId, { active: true }).catch(() => null);
      if (tab) await chrome.windows.update(tab.windowId, { focused: true }).catch(() => {});
      return;
    }
    chrome.tabs.sendMessage(tabId, { type: "media-control", action: message.action, value: message.value }).catch(() => {});
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

// ── YouTube Music's player (media.js), for Ciel's YouTube Music pill ─────────

/** The tabs where YouTube Music plays or has played, by id. */
const mediaTabs = new Set();
/** Covers already made small: the page's URL → a JPEG data URL. */
const covers = new Map();
const COVER = 120;

/** The cover as a small JPEG data URL (made once per URL), or null. */
async function coverData(url) {
  if (!url || !/^https:/.test(url)) return null;
  if (covers.has(url)) return covers.get(url);
  let data = null;
  try {
    const blob = await (await fetch(url, { credentials: "omit" })).blob();
    const image = await createImageBitmap(blob);
    // Square, cropped to the middle: a video's cover is 16:9.
    const side = Math.min(image.width, image.height);
    const canvas = new OffscreenCanvas(COVER, COVER);
    canvas.getContext("2d").drawImage(image, (image.width - side) / 2, (image.height - side) / 2, side, side, 0, 0, COVER, COVER);
    const jpeg = await canvas.convertToBlob({ type: "image/jpeg", quality: 0.85 });
    data = `data:image/jpeg;base64,${base64(await jpeg.arrayBuffer())}`;
  } catch {
    data = null;
  }
  covers.set(url, data);
  while (covers.size > 20) covers.delete(covers.keys().next().value);
  return data;
}

async function forwardMedia(message, tab) {
  connect();
  if (!port || !ciel) return;
  if (message.gone) {
    mediaTabs.delete(tab.id);
    port.postMessage({ type: "media", key: String(tab.id), gone: true });
    return;
  }
  mediaTabs.add(tab.id);
  port.postMessage({
    type: "media",
    key: String(tab.id),
    title: String(message.title ?? "").slice(0, 300),
    artist: String(message.artist ?? "").slice(0, 300),
    album: String(message.album ?? "").slice(0, 300),
    art: await coverData(message.art),
    duration: Number(message.duration) || 0,
    position: Number(message.position) || 0,
    playing: message.playing === true,
  });
}

/** A YouTube Music tab closed, or gone elsewhere: its player with it. */
function mediaGone(tabId) {
  if (!mediaTabs.delete(tabId)) return;
  port?.postMessage({ type: "media", key: String(tabId), gone: true });
}

chrome.tabs.onRemoved.addListener((tabId) => mediaGone(tabId));
chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.url && !change.url.startsWith("https://music.youtube.com/")) mediaGone(tabId);
});

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (message?.type === "hello") {
    connect();
    reply({ ciel });
    return;
  }
  if (message?.type === "notification" && sender.tab?.id != null) void forward(message, sender.tab);
  if (message?.type === "media" && sender.tab?.id != null && sender.tab.url?.startsWith("https://music.youtube.com/")) {
    void forwardMedia(message, sender.tab);
  }
});

connect();
