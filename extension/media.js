// Ciel for Chrome — YouTube Music's player, for Ciel's YouTube Music pill. In
// the page's own world on music.youtube.com, from the start: the page hands
// its media session the handlers the media keys use (play, pause, next…), and
// keeps a note of them on the way, so Ciel's buttons do exactly what the media
// keys do. What plays is read from the media session's metadata and the
// player's <video>, and sent up to relay.js (then sw.js, then Ciel) when it
// changes — a new track, play, pause, a seek — and every 15 s while it plays,
// to keep the clock right. Nothing else on the page is read.

(() => {
  const TO_RELAY = "ciel:to-relay";
  const TO_PAGE = "ciel:to-page";
  const ms = navigator.mediaSession;
  if (!ms) return;

  /** The page's own handlers, by action. */
  const handlers = new Map();
  const setActionHandler = ms.setActionHandler.bind(ms);
  ms.setActionHandler = (action, handler) => {
    if (typeof handler === "function") handlers.set(action, handler);
    else handlers.delete(action);
    return setActionHandler(action, handler);
  };

  const here = location.origin;
  const post = (message) => window.postMessage({ [TO_RELAY]: true, ...message }, here);
  const video = () => document.querySelector("video");

  /** The largest cover the page offers, for sw.js to make small. */
  function cover(metadata) {
    const size = (a) => Math.max(0, ...String(a.sizes ?? "").split(/\s+/).map((s) => parseInt(s, 10) || 0));
    const art = [...(metadata.artwork ?? [])].sort((a, b) => size(b) - size(a))[0];
    return art?.src ?? null;
  }

  let sent = "";
  let sentAt = 0;
  function report(force) {
    const m = ms.metadata;
    const v = video();
    if (!m || !m.title || !v) {
      if (sent !== "none") {
        sent = "none";
        post({ type: "media", gone: true });
      }
      return;
    }
    const duration = Number.isFinite(v.duration) ? v.duration : 0;
    const state = {
      title: String(m.title).slice(0, 300),
      artist: String(m.artist ?? "").slice(0, 300),
      album: String(m.album ?? "").slice(0, 300),
      art: cover(m),
      duration,
      position: v.currentTime || 0,
      playing: !v.paused && !v.ended,
    };
    // The position runs on by itself while it plays: only a jump is news.
    const key = JSON.stringify([state.title, state.artist, state.album, state.art, Math.round(duration), state.playing]);
    const stale = state.playing && Date.now() - sentAt > 15_000;
    if (!force && !stale && key === sent) return;
    sent = key;
    sentAt = Date.now();
    post({ type: "media", ...state });
  }

  // Media events don't bubble, but a capturing listener sees them all, the
  // <video> being swapped or not.
  for (const event of ["play", "pause", "ended", "seeked", "loadedmetadata", "durationchange"]) {
    document.addEventListener(event, (e) => {
      if (e.target instanceof HTMLMediaElement) report(true);
    }, true);
  }
  // A new track's metadata comes without an event of its own.
  window.setInterval(() => report(false), 2000);

  const click = (selector) => {
    const button = document.querySelector(selector);
    if (button) button.click();
    return !!button;
  };
  const call = (action, details = {}) => {
    const handler = handlers.get(action);
    if (!handler) return false;
    handler({ action, ...details });
    return true;
  };

  window.addEventListener("message", (e) => {
    if (e.source !== window || !e.data || e.data[TO_PAGE] !== true || e.data.type !== "media-control") return;
    const v = video();
    switch (e.data.action) {
      case "playPause":
        if (v?.paused) call("play") || click(".play-pause-button") || v.play();
        else call("pause") || click(".play-pause-button") || v?.pause();
        break;
      case "next":
        call("nexttrack") || click(".next-button");
        break;
      case "previous":
        call("previoustrack") || click(".previous-button");
        break;
      case "seek": {
        const to = Number(e.data.value);
        if (!Number.isFinite(to)) break;
        if (!call("seekto", { seekTime: to, fastSeek: false }) && v) v.currentTime = to;
        break;
      }
      case "report":
        report(true);
        break;
    }
    window.setTimeout(() => report(true), 300);
  });
})();
