// Ciel for Chrome — in the page's own world, before its scripts run. While
// Ciel is there, the notifications the page shows go to Ciel's island instead
// of the browser's popup; a click in Ciel reaches the page as the popup's
// would. Without Ciel, or without the site's permission to notify, the
// browser shows them exactly as always. Messages go through relay.js, in this
// tab only.

(() => {
  const Native = window.Notification;
  if (typeof Native !== "function" || window.__cielNotifications) return;
  window.__cielNotifications = true;

  const TO_RELAY = "ciel:to-relay";
  const TO_PAGE = "ciel:to-page";
  const here = location.origin === "null" ? "*" : location.origin;
  let ciel = false;
  let seq = 0;
  /** The page's notifications shown in Ciel, by key: their click and close come back here. */
  const live = new Map();

  const send = (message) => window.postMessage({ [TO_RELAY]: true, ...message }, here);

  window.addEventListener("message", (e) => {
    if (e.source !== window || !e.data || e.data[TO_PAGE] !== true) return;
    const m = e.data;
    if (m.type === "status") {
      ciel = m.on === true;
    } else if (m.type === "click" || m.type === "close") {
      const n = live.get(m.key);
      if (!n) return;
      if (m.type === "close") live.delete(m.key);
      n._fire(m.type);
    }
  });

  const absolute = (url) => {
    try {
      return url ? new URL(String(url), location.href).href : "";
    } catch {
      return "";
    }
  };

  /** Hands a notification to Ciel; its key, for what comes back. */
  function toCiel(title, options) {
    const key = `${Date.now().toString(36)}.${++seq}`;
    send({
      type: "notification",
      key,
      title: String(title ?? ""),
      body: String(options?.body ?? ""),
      icon: absolute(options?.icon || options?.badge || ""),
      tag: String(options?.tag ?? ""),
    });
    return key;
  }

  const useCiel = () => ciel && Native.permission === "granted";

  class CielNotification extends EventTarget {
    constructor(title, options = {}) {
      super();
      // No Ciel, or no permission: the browser's own, as the page expects.
      if (!useCiel()) return new Native(title, options);
      this.title = String(title ?? "");
      this.body = String(options.body ?? "");
      this.tag = String(options.tag ?? "");
      this.icon = absolute(options.icon);
      this.data = options.data ?? null;
      this.silent = options.silent ?? null;
      this.onclick = null;
      this.onclose = null;
      this.onshow = null;
      this.onerror = null;
      this._key = toCiel(title, options);
      live.set(this._key, this);
      setTimeout(() => this._fire("show"));
    }

    _fire(type) {
      const event = new Event(type);
      this.dispatchEvent(event);
      const handler = this[`on${type}`];
      if (typeof handler === "function") handler.call(this, event);
    }

    close() {
      if (!live.delete(this._key)) return;
      send({ type: "close", key: this._key });
      this._fire("close");
    }

    static get permission() {
      return Native.permission;
    }

    static requestPermission(callback) {
      return Native.requestPermission(callback);
    }

    static get maxActions() {
      return Native.maxActions;
    }
  }

  Object.defineProperty(window, "Notification", { value: CielNotification, writable: true, configurable: true });

  // What the page shows through its service worker registration goes the same way.
  const registration = window.ServiceWorkerRegistration && window.ServiceWorkerRegistration.prototype;
  if (registration && typeof registration.showNotification === "function") {
    const show = registration.showNotification;
    registration.showNotification = function (title, options = {}) {
      if (!useCiel()) return show.call(this, title, options);
      toCiel(title, options);
      return Promise.resolve();
    };
  }
})();
