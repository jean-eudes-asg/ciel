// The Printers pill in the island and its words (src/core/printers.ts,
// src/views/printers.ts): what Rust sends is checked, problems read as the
// report says them, and the card and detail do what their buttons say.

import { test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { sent } from "./tauri.mjs";

installFakeDom();

const {
  isPrinterHost, lowestSupply, newPrinterId, printerColor, printerLine, problemText, readPrinters, supplyColor,
} = await import("../src/core/printers.ts");
const { printersCard, printersDetail } = await import("../src/views/printers.ts");
const { parseNotification } = await import("../src/core/notifications.ts");

const canon = {
  id: "p-1",
  name: "Canon 2F",
  host: "192.168.1.237",
  location: "2nd floor",
  model: "Canon MF650C Series",
  serial: "",
  panel: "",
  state: "idle",
  via: "ipp",
  problems: [
    { code: "jammed", severity: "critical", detail: "", sub: "" },
    { code: "supplyLow", severity: "warning", detail: "Cyan Toner 9%", sub: "Cyan Toner" },
  ],
  supplies: [
    { name: "Black Toner", kind: "toner", color: "#000000", level: 57 },
    { name: "Cyan Toner", kind: "toner", color: "#00FFFF", level: 9 },
  ],
  trays: [{ name: "Drawer 1", level: 30, empty: false }],
  pages: 48210,
  checkedAt: Date.now() - 120_000,
  since: 0,
  incidents: 3,
  web: "http://192.168.1.237/",
  worst: "critical",
};
const kyocera = {
  ...canon,
  id: "p-2",
  name: "Kyocera",
  host: "192.168.1.233",
  problems: [],
  supplies: [{ name: "TK-3100", kind: "toner", color: "#000000", level: 48 }],
  worst: null,
  web: "javascript:alert(1)",
};

test("what Rust sends is checked: a printer needs an id, a bad page is no page", () => {
  const list = readPrinters({ printers: [canon, kyocera, { name: "no id" }, null] });
  assert.equal(list.length, 2);
  assert.equal(list[1].web, "", "only http(s) pages open");
  assert.deepEqual(readPrinters(undefined), []);
  assert.deepEqual(readPrinters({ printers: "nope" }), []);
  const sparse = readPrinters({ printers: [{ id: "p-3" }] })[0];
  assert.equal(sparse.state, "unknown");
  assert.deepEqual(sparse.problems, []);
  assert.equal(sparse.pages, null);
});

test("problems read as the report says them", () => {
  assert.equal(problemText({ code: "jammed", severity: "critical", detail: "", sub: "" }), "Paper jam");
  assert.equal(problemText({ code: "supplyLow", severity: "warning", detail: "Cyan Toner 9%", sub: "" }), "Cyan Toner 9%");
  assert.equal(problemText({ code: "serviceRequested", severity: "critical", detail: "Replace fuser", sub: "" }), "Service requested (Replace fuser)");
  assert.equal(problemText({ code: "brand new", severity: "warning", detail: "", sub: "" }), "Printer alert");
});

test("colours: the worst problem, green when fine, grey before it's known; black toner shows light", () => {
  const [c, k] = readPrinters({ printers: [canon, kyocera] });
  assert.equal(printerColor(c), "#F4505E");
  assert.equal(printerColor(k), "#22C55E");
  assert.equal(printerColor({ ...k, state: "waiting" }), "#6B7079");
  assert.equal(supplyColor({ name: "K", kind: "toner", color: "#000000", level: 1 }), "#C5C8CD");
  assert.equal(supplyColor({ name: "C", kind: "toner", color: "#00FFFF", level: 1 }), "#00FFFF");
  assert.equal(supplyColor({ name: "?", kind: "toner", color: "red", level: 1 }), "#9398A1");
});

test("a printer's line: its problems, else its state and its emptiest cartridge", () => {
  const [c, k] = readPrinters({ printers: [canon, kyocera] });
  assert.equal(printerLine(c), "Paper jam · Cyan Toner 9%");
  assert.equal(printerLine(k), "Idle · TK-3100 48%");
  assert.equal(lowestSupply(c).name, "Cyan Toner");
});

test("ids are new each time and addresses are only addresses", () => {
  assert.match(newPrinterId(1_761_000_000_000, 0.5), /^p-[0-9a-z]+-[0-9a-z]{4}$/);
  assert.notEqual(newPrinterId(1, 0.1), newPrinterId(1, 0.2));
  for (const ok of ["192.168.1.40", "printer.local", "fe80::1", "[fe80::1]", "BRN30055C"]) assert.ok(isPrinterHost(ok), ok);
  for (const bad of ["", "http://10.0.0.1", "10.0.0.1/24", "a b", "-x"]) assert.ok(!isPrinterHost(bad), bad);
});

test("the card lists the printers; a press opens the detail; none sends to Settings", () => {
  let detail = 0;
  let settings = 0;
  const card = printersCard({ printers: [canon, kyocera] }, 3, () => detail++, () => settings++);
  assert.ok(card.textContent.includes("Printers"));
  assert.ok(card.textContent.includes("1/2"), "one of two has a problem");
  const rows = card.find(".int-row");
  assert.equal(rows.length, 2);
  assert.ok(rows[0].textContent.includes("Canon 2F"));
  assert.ok(rows[0].textContent.includes("Paper jam"));
  rows[1].fire("click");
  assert.equal(detail, 1);

  const more = printersCard({ printers: [canon, kyocera, { ...kyocera, id: "p-3" }, { ...kyocera, id: "p-4" }] }, 3, () => {}, () => {});
  assert.ok(more.textContent.includes("+1 more"));

  const empty = printersCard({ printers: [] }, 3, () => {}, () => settings++);
  assert.ok(empty.textContent.includes("Add printers in Settings"));
  empty.find(".int-row")[0].fire("click");
  assert.equal(settings, 1);
});

test("the detail has every cartridge as a bar, Check now, and the printer's page", () => {
  const detail = printersDetail({ printers: [canon, kyocera] }, () => {});
  const text = detail.textContent;
  assert.ok(text.includes("Black Toner") && text.includes("57%"));
  assert.ok(text.includes("Drawer 1"));
  assert.ok(text.includes("48210 pages"));
  assert.ok(text.includes("3 problems in 30 days"));
  // The low cartridge's level stands out.
  assert.equal(detail.find(".low").length, 1);

  detail.find(".pr-check")[0].fire("click");
  assert.equal(sent("printers_check_now").length, 1);
  const links = detail.find(".int-link").filter((b) => b.textContent === "Open its web page");
  assert.equal(links.length, 1, "Kyocera's page isn't a web page");
  links[0].fire("click");
  assert.deepEqual(sent("open_url").at(-1), { url: "http://192.168.1.237/" });
});

test("a printer's problem comes as a notification of its own source", () => {
  const n = parseNotification({ id: "printers-1", source: "printers", app: "Canon 2F", title: "Paper jam", body: "2nd floor", icon: null, at: 1, tag: "printer:p-1:jammed:" });
  assert.equal(n.source, "printers");
  assert.equal(parseNotification({ id: "printers-2", source: "printers", app: "", title: "x", body: "", icon: null, at: 1, tag: null }).app, "Printers");
});
