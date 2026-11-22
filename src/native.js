// iOS: the chrome is SwiftUI with Liquid Glass (src-tauri/plugins/native-ui): title bar, keys above the keyboard,
// menus, dialogs and toasts. The page keeps the terminal and the lists. Dialogs are still built as DOM, the same
// code as on the desktop: the DOM stays hidden, SwiftUI shows its controls and writes the user's input back into it.
import { invoke, Channel } from "@tauri-apps/api/core";

const call = (cmd, args) => invoke("plugin:native-ui|" + cmd, args);
export const native = { on: false };

/** SF Symbols for the icon names of ui.js. */
export const SF = {
  connect: "powerplug", terminal: "apple.terminal", tmux: "square.stack.3d.up", edit: "pencil", copy: "doc.on.doc",
  star: "star", unstar: "star.slash", trash: "trash", plus: "plus", folderPlus: "folder.badge.plus", cut: "scissors",
  paste: "doc.on.clipboard", selectAll: "selection.pin.in.out", clear: "eraser", splitH: "rectangle.split.2x1",
  splitV: "rectangle.split.1x2", close: "xmark", download: "arrow.down.circle", upload: "arrow.up.circle", up: "arrow.up",
  refresh: "arrow.clockwise", folder: "folder", folderTree: "folder", stats: "waveform.path.ecg", docker: "shippingbox",
  syncOk: "checkmark.icloud", syncOff: "xmark.icloud", syncConflict: "exclamationmark.icloud", syncing: "arrow.triangle.2.circlepath.icloud",
  // Host icons (ui.js HOST_ICONS)
  server: "server.rack", cloud: "cloud", database: "cylinder.split.1x2", desktop: "desktopcomputer", board: "cpu",
  router: "wifi.router", firewall: "shield", web: "globe", home: "house", box: "cube.box", nas: "externaldrive",
  prod: "flame", deploy: "paperplane",
};

let chrome = () => ({});
let chromeTimer = 0, chromeLast = "";

/** `state()` gives what the title bar shows; actions and keys come back to `onAction(id)` / `onKey(key, text)`. */
export async function initNative(state, onAction, onKey) {
  const events = new Channel();
  events.onmessage = (m) => (m.sheet != null ? sheetEvent(m) : m.key ? onKey(m.key, m.text) : m.action && onAction(m.action));
  chrome = state;
  await call("attach", { events });
  native.on = true;
  document.documentElement.classList.add("native"); // style.css: the HTML title bar gives way
  refreshChrome();
}

/** Something the title bar shows changed: sent once per turn, and only if different. */
export function refreshChrome() {
  if (!native.on || chromeTimer) return;
  chromeTimer = setTimeout(() => {
    chromeTimer = 0;
    const state = chrome(), json = JSON.stringify(state);
    if (json !== chromeLast) { chromeLast = json; call("chrome", { state }).catch(() => {}); }
  });
}

/** Menu at a point (long press); items as for ui.js contextMenu. */
export async function nativeMenu(x, y, items) {
  const list = items.filter(Boolean);
  const { index } = await call("menu", { x, y, items: list.map((it) => (it === "-" ? { sep: true }
    : { l: it.label, sf: SF[it.icon] || "circle", danger: !!it.danger, disabled: !!it.disabled })) });
  list[index]?.action?.();
}

export const nativeToast = (text, kind) => call("toast", { text: String(text), kind }).catch(() => {});

// ---------- dialogs ----------

const sheets = new Map();
let seq = 0, nid = 0;

/** Same contract as ui.js modal(): resolves with what `close()` gets (undefined when swiped away). */
export function nativeModal(title, build, { wide = false, cls = "" } = {}) {
  document.activeElement?.blur?.(); // or the keyboard stays up over the sheet
  return new Promise((resolve) => {
    const id = ++seq;
    // Hidden (style.css), but still there for the code that looks for an open dialog (.backdrop, .settings-modal).
    const root = document.createElement("div");
    root.className = "backdrop native";
    const box = document.createElement("div");
    box.className = ("modal " + cls).trim();
    root.append(box);
    const s = { id, title, wide, root, nodes: new Map(), last: "", timer: 0 };
    s.close = (v) => {
      if (!sheets.delete(id)) return;
      s.obs.disconnect();
      root.remove();
      call("dismiss", { id }).catch(() => {});
      resolve(v);
    };
    sheets.set(id, s);
    box.append(build(s.close));
    document.body.append(root);
    s.obs = new MutationObserver(() => push(s));
    s.obs.observe(root, { subtree: true, childList: true, attributes: true, characterData: true });
    push(s);
  });
}

function push(s) {
  clearTimeout(s.timer);
  s.timer = setTimeout(() => {
    if (!sheets.has(s.id)) return;
    const sheet = serialize(s), json = JSON.stringify(sheet);
    if (json !== s.last) { s.last = json; call("sheet", { sheet }).catch(() => {}); }
  });
}

function sheetEvent(m) {
  const s = sheets.get(m.sheet);
  if (!s) return;
  if (m.dismissed || m.close) return s.close(undefined);
  const el = s.nodes.get(m.click ?? m.change);
  if (!el) return;
  if (m.click != null) el.click();
  else {
    if (el.type === "checkbox") el.checked = m.value === "true";
    else el.value = m.value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }
  push(s); // property changes (value, checked) are not mutations
}

const shown = (el) => !el.hidden && !el.closest("[hidden]");
const text = (el) => (el?.textContent || "").replace(/\s+/g, " ").trim();
/** An element's words without its <small> hint. */
const own = (el) => {
  if (!el) return "";
  const c = el.cloneNode(true);
  c.querySelectorAll("small").forEach((x) => x.remove());
  return text(c);
};

/** The dialog's DOM as sections of rows (Sheet.swift); each row keeps the id of the element it mirrors. */
function serialize(s) {
  s.nodes.clear();
  const key = (el) => { el._nid ??= ++nid; s.nodes.set(el._nid, el); return el._nid; };
  const doc = { id: s.id, title: s.title, large: s.wide, tabs: [], sections: [{ id: 0, rows: [] }], actions: [] };
  const add = (row) => row && doc.sections.at(-1).rows.push(row);

  const control = (el, label, hint) => {
    const base = { id: key(el), label: label || undefined, hint: hint || undefined, disabled: el.disabled || undefined };
    if (el.tagName === "BUTTON") return { ...base, k: "button", label: text(el) || el.getAttribute("aria-label") || "", hint: undefined, role: el.classList.contains("primary") ? "primary" : undefined };
    if (el.tagName === "SELECT") return { ...base, k: "select", text: el.value, options: [...el.options].map((o) => ({ v: o.value, l: o.textContent })) };
    if (el.tagName === "TEXTAREA") return { ...base, k: "multiline", text: el.value, placeholder: el.placeholder };
    const num = (v) => (v === "" ? undefined : +v);
    switch (el.type) {
      case "checkbox": return { ...base, k: "toggle", on: el.checked };
      case "range": {
        const out = el.parentElement.querySelector("output");
        return { ...base, k: "slider", text: el.value, min: num(el.min), max: num(el.max), step: +el.step || 1, unit: out ? out.value.slice(el.value.length) : "" };
      }
      case "color": return { ...base, k: "color", text: el.value };
      case "number": return { ...base, k: "number", text: el.value, placeholder: el.placeholder, min: num(el.min), max: num(el.max), step: +el.step || 1 };
      case "password": return { ...base, k: "secure", text: el.value, placeholder: el.placeholder };
      default: return { ...base, k: "field", text: el.value, placeholder: el.placeholder, keyboard: el.inputMode || (el.type === "url" ? "url" : undefined) };
    }
  };

  const walk = (el) => {
    for (const c of el.children) {
      if (!shown(c) || c.matches("datalist, img, output, h3")) continue;
      if (c.matches("[role=tablist]")) {
        doc.tabs = [...c.querySelectorAll("button")].map((b) => ({ id: key(b), l: text(b), on: b.getAttribute("aria-selected") === "true" }));
      } else if (c.matches("footer")) {
        // Cancel / Save…: the sheet's toolbar. The note next to them (config file path) is desktop matter.
        for (const b of c.querySelectorAll("button")) {
          const submit = b.type === "submit";
          doc.actions.push({ id: key(b), l: text(b), primary: submit || b.classList.contains("primary"), disabled: submit && b.form ? !b.form.checkValidity() : undefined });
        }
      } else if (c.matches("h2, h4")) {
        doc.sections.push({ id: key(c), header: own(c), rows: [] });
        const note = c.querySelector("small");
        if (note) add({ id: key(note), k: "note", label: text(note).replace(/^—\s*/, "") });
      } else if (c.matches("label.field")) {
        const label = own(c.querySelector(":scope > span")), hint = text(c.querySelector(":scope > small"));
        const grid = c.querySelector(".icon-grid");
        if (grid) {
          add({ id: key(grid), k: "icons", label, options: [...grid.children].map((b) => ({ id: key(b), v: b.title, l: b.getAttribute("aria-label"), sf: SF[b.title], on: b.classList.contains("on") })) });
          continue;
        }
        let first = true;
        for (const x of c.querySelectorAll("input, select, textarea, button")) {
          if (!shown(x)) continue;
          if (x.tagName === "BUTTON") { add(control(x)); continue; }
          add(first ? control(x, label, hint) : control(x, x.getAttribute("aria-label") || x.placeholder));
          first = false;
        }
      } else if (c.matches(".toggle")) {
        const span = c.querySelector(":scope > span"), color = c.querySelector("input[type=color]"), sw = c.querySelector("input[type=checkbox]");
        if (color) add({ ...control(color, own(span)), toggle: sw && key(sw), on: sw?.checked });
        else if (sw) add(control(sw, own(span), text(span?.querySelector("small"))));
      } else if (c.matches(".item")) {
        add({ id: key(c), k: "item", label: text(c.querySelector("strong, .item-name")) || own(c),
          detail: [...c.querySelectorAll(".item-meta, .conflict-diff li")].map(text).join("\n") || undefined,
          buttons: [...c.querySelectorAll("button")].filter(shown).map((b) => control(b)) });
      } else if (c.matches("dl")) {
        for (const dt of c.querySelectorAll("dt")) add({ id: key(dt), k: "pair", label: text(dt), text: text(dt.nextElementSibling) });
      } else if (c.matches("input, select, textarea, button")) {
        add(control(c, c.getAttribute("aria-label") || c.placeholder));
      } else if (c.matches("p, small, span, strong, h1, li") && !c.querySelector("input, select, textarea, button")) {
        const t = text(c);
        if (t) add({ id: key(c), k: c.matches(".error, [role=alert]") ? "error" : c.matches("h1") ? "title" : "note", label: t });
      } else walk(c);
    }
  };
  walk(s.root);
  doc.sections = doc.sections.filter((x) => x.rows.length);
  return doc;
}
