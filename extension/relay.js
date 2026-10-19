// Ciel for Chrome — between the page (page.js, in the page's own world) and
// the extension's worker (sw.js): the page's notifications go up, Ciel's
// answers (whether it is there, a click, a close) come down.

(() => {
  const TO_RELAY = "ciel:to-relay";
  const TO_PAGE = "ciel:to-page";
  const here = location.origin === "null" ? "*" : location.origin;
  const toPage = (message) => window.postMessage({ [TO_PAGE]: true, ...message }, here);

  window.addEventListener("message", (e) => {
    if (e.source !== window || !e.data || e.data[TO_RELAY] !== true) return;
    const { type, key, title, body, icon, tag } = e.data;
    if (type === "notification" || type === "close") {
      chrome.runtime.sendMessage({ type, key, title, body, icon, tag }).catch(() => {});
    }
  });

  chrome.runtime.onMessage.addListener((m) => {
    if (m && (m.type === "status" || m.type === "click" || m.type === "close")) toPage(m);
  });

  // Is Ciel there? Asked as the page starts; the worker tells every tab when it changes.
  chrome.runtime
    .sendMessage({ type: "hello" })
    .then((reply) => toPage({ type: "status", on: reply?.ciel === true }))
    .catch(() => {});
})();
