// Tabs, split panes (tmux-like) and xterm terminals bound to SSH sessions.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { S, h, icon, invoke, modal, field, toast } from "./ui.js";

// Layout tree per tab: { split: "h"|"v", ratio, a, b } | Pane
const tabs = [];
let current = null;
let zoom = 0;
let xtheme = {};
const bySession = new Map();
export const events = new EventTarget(); // "focus" when the focused pane/session changes

class Pane {
  constructor() {
    this.el = h("div.pane", { onmousedown: () => focusPane(this) });
    this.session = null;
    this.host = null;
    this.term = null;
    this.picker();
  }
  picker() {
    const list = h("div.picker-list");
    const input = h("input", { placeholder: "Rechercher un hôte…", "aria-label": "Rechercher un hôte" });
    const draw = () => {
      const q = input.value.toLowerCase();
      list.replaceChildren(...S.hosts.hosts.filter((x) => !q || (x.name + x.host + x.tags.join(" ")).toLowerCase().includes(q)).slice(0, 30)
        .map((x) => h("button.row.host", { onclick: () => this.connect(x) }, icon(x.icon || "server"), h("span.name", {}, x.name), h("span.mute", {}, x.host))));
    };
    input.oninput = draw;
    input.onkeydown = (e) => { if (e.key === "Enter") list.querySelector("button")?.click(); };
    draw();
    this.el.replaceChildren(h("div.picker", {}, h("h3", {}, "Ouvrir une connexion"), input, list));
    this.focusTarget = input;
  }
  async connect(host, password = null) {
    this.host = host;
    this.el.replaceChildren(h("div.connecting", { role: "status" }, h("span.dot"), `Connexion à ${host.name}…`));
    const t = S.settings.terminal;
    const term = new Terminal({
      fontFamily: t.font_family, fontSize: t.font_size + zoom, lineHeight: t.line_height, cursorStyle: t.cursor_style,
      cursorBlink: t.cursor_blink, scrollback: t.scrollback, theme: xtheme, allowProposedApi: true,
      macOptionIsMeta: true, rightClickSelectsWord: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const ch = new Channel();
    ch.onmessage = (buf) => term.write(new Uint8Array(buf));
    try {
      this.session = await invoke("ssh_connect", { hostId: host.id, cols: 80, rows: 24, password, onData: ch });
    } catch (e) {
      term.dispose();
      if (String(e).includes("NEED_PASSWORD")) return this.askPassword(host);
      return this.failed(host, String(e));
    }
    bySession.set(this.session, this);
    this.term = term;
    this.fit = fit;
    const box = h("div.term");
    this.el.replaceChildren(box);
    term.open(box);
    term.onData((d) => invoke("ssh_write", { id: this.session, data: d }).catch(() => {}));
    term.onResize(({ cols, rows }) => invoke("ssh_resize", { id: this.session, cols, rows }).catch(() => {}));
    term.onSelectionChange(() => S.settings.terminal.copy_on_select && term.hasSelection() && navigator.clipboard.writeText(term.getSelection()));
    term.attachCustomKeyEventHandler((e) => {
      // Ctrl+Shift+C/V on Linux/Windows (Cmd+C/V on macOS come from the native Edit menu).
      if (e.type === "keydown" && e.ctrlKey && e.shiftKey && !isMacPlatform()) {
        if (e.code === "KeyC") { navigator.clipboard.writeText(term.getSelection()); return false; }
        if (e.code === "KeyV") { navigator.clipboard.readText().then((t) => term.paste(t)); return false; }
      }
      return true;
    });
    // Defer to the next frame: fitting inside the observer callback resizes the box again and loops.
    this.ro = new ResizeObserver(() => requestAnimationFrame(() => this.refit()));
    this.ro.observe(box);
    this.focusTarget = term;
    this.refit();
    if (current?.focused === this) { term.focus(); events.dispatchEvent(new Event("focus")); }
    updateTabTitles();
  }
  async askPassword(host) {
    const pw = h("input", { type: "password", autofocus: true, autocomplete: "off" });
    const remember = h("input", { type: "checkbox" });
    const ok = await modal(`Mot de passe pour ${host.user || ""}@${host.host}`, (close) =>
      h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field("Mot de passe", pw),
        h("label.check", {}, remember, "Mémoriser dans le trousseau du système"),
        h("footer", {}, h("button.btn", { type: "button", onclick: () => close(false) }, "Annuler"), h("button.btn.primary", { type: "submit" }, "Se connecter"))));
    if (!ok) return this.picker();
    if (remember.checked) await invoke("set_secret", { id: host.id, value: pw.value }).catch((e) => toast(String(e), "error"));
    this.connect(host, pw.value);
  }
  failed(host, msg) {
    this.el.replaceChildren(h("div.picker", {}, h("h3.error", {}, `Échec : ${host.name}`), h("p.mute", {}, msg),
      h("div.inline", {}, h("button.btn.primary", { onclick: () => this.connect(host) }, "Réessayer"), h("button.btn", { onclick: () => this.picker() }, "Autre hôte"))));
  }
  closed() {
    bySession.delete(this.session);
    this.session = null;
    this.term?.write("\r\n\x1b[2m[connexion fermée — Entrée pour se reconnecter]\x1b[0m\r\n");
    const t = this.term;
    const sub = t?.onKey(({ domEvent }) => { if (domEvent.key === "Enter") { sub.dispose(); this.dispose(); this.connect(this.host); } });
    events.dispatchEvent(new Event("focus"));
  }
  refit() {
    if (!this.fit || !this.el.isConnected) return;
    try { this.fit.fit(); } catch {}
  }
  dispose() {
    if (this.session) invoke("ssh_close", { id: this.session });
    bySession.delete(this.session);
    this.ro?.disconnect();
    this.term?.dispose();
    this.term = this.fit = null;
  }
}

const isMacPlatform = () => S.platform === "macos";
const leaves = (n) => (n instanceof Pane ? [n] : [...leaves(n.a), ...leaves(n.b)]);

function renderNode(n) {
  if (n instanceof Pane) return n.el;
  const a = h("div.cell", {}, renderNode(n.a));
  const b = h("div.cell", {}, renderNode(n.b));
  a.style.flex = `${n.ratio} 1 0`;
  b.style.flex = `${1 - n.ratio} 1 0`;
  const gutter = h("div.gutter", { role: "separator", "aria-orientation": n.split === "h" ? "vertical" : "horizontal",
    onmousedown: (e) => {
      e.preventDefault();
      const box = gutter.parentElement.getBoundingClientRect();
      const move = (ev) => {
        n.ratio = Math.min(0.9, Math.max(0.1, n.split === "h" ? (ev.clientX - box.left) / box.width : (ev.clientY - box.top) / box.height));
        a.style.flex = `${n.ratio} 1 0`; b.style.flex = `${1 - n.ratio} 1 0`;
      };
      const up = () => { removeEventListener("mousemove", move); removeEventListener("mouseup", up); };
      addEventListener("mousemove", move); addEventListener("mouseup", up);
    } });
  return h("div.split." + n.split, {}, a, gutter, b);
}

function render() {
  const strip = document.getElementById("tabs");
  strip.replaceChildren(...tabs.map((t, i) => h("div.tab" + (t === current ? ".on" : ""), { role: "tab", "aria-selected": String(t === current) },
    h("button.tab-label", { onclick: () => selectTab(t), onauxclick: (e) => e.button === 1 && closeTab(t) }, t.title()),
    h("button.icon-btn", { "aria-label": "Fermer l'onglet " + (i + 1), onclick: () => closeTab(t) }, icon("close", 12)))),
    h("button.icon-btn.new-tab", { "aria-label": "Nouvel onglet", title: "Nouvel onglet", onclick: () => newTab() }, icon("plus", 14)));
  const area = document.getElementById("panes");
  area.replaceChildren(current ? renderNode(current.root) : h("div.welcome", {}, h("h1", {}, "RUSTTERM", h("span.dot-end", {}, ".")), h("p", {}, "Choisissez un hôte dans la barre latérale.")));
  current && leaves(current.root).forEach((p) => p.el.classList.toggle("focused", p === current.focused));
}

function updateTabTitles() { render(); }

export function newTab(host = null) {
  const p = new Pane();
  const tab = { root: p, focused: p, title() { const l = leaves(this.root); const n = l[0].host?.name || "Nouvel onglet"; return l.length > 1 ? `${n} +${l.length - 1}` : n; } };
  tabs.push(tab);
  selectTab(tab);
  if (host) p.connect(host);
  return p;
}

function selectTab(t) {
  current = t;
  render();
  focusPane(t.focused);
}

function closeTab(t) {
  leaves(t.root).forEach((p) => p.dispose());
  const i = tabs.indexOf(t);
  tabs.splice(i, 1);
  current = tabs[Math.min(i, tabs.length - 1)] || null;
  render();
  if (current) focusPane(current.focused);
  else events.dispatchEvent(new Event("focus"));
}

export function focusPane(p) {
  if (!current) return;
  current.focused = p;
  leaves(current.root).forEach((x) => x.el.classList.toggle("focused", x === p));
  requestAnimationFrame(() => p.focusTarget?.focus());
  events.dispatchEvent(new Event("focus"));
}

export const focused = () => current?.focused || null;

/** Open a host: in the focused pane if it's empty, else a new tab. */
export function openHost(host, forceTab = false) {
  const p = focused();
  if (!forceTab && p && !p.session && !p.term) return p.connect(host);
  newTab(host);
}

export function split(dir) {
  if (!current) return newTab();
  const old = current.focused;
  const p = new Pane();
  const horizontal = dir === "right" || dir === "left";
  const first = dir === "right" || dir === "down";
  const node = { split: horizontal ? "h" : "v", ratio: 0.5, a: first ? old : p, b: first ? p : old };
  replace(current, old, node);
  render();
  focusPane(p);
}

function replace(tab, target, node) {
  if (tab.root === target) return (tab.root = node);
  const walk = (n) => {
    if (n instanceof Pane) return;
    if (n.a === target) n.a = node; else if (n.b === target) n.b = node; else { walk(n.a); walk(n.b); }
  };
  walk(tab.root);
}

export function closePane() {
  if (!current) return;
  const p = current.focused;
  if (current.root === p) return closeTab(current);
  const parentOf = (n) => (n instanceof Pane ? null : n.a === p || n.b === p ? n : parentOf(n.a) || parentOf(n.b));
  const parent = parentOf(current.root);
  const sibling = parent.a === p ? parent.b : parent.a;
  replace(current, parent, sibling);
  p.dispose();
  current.focused = leaves(sibling)[0];
  render();
  focusPane(current.focused);
}

export function cycleFocus(step) {
  if (!current) return;
  const l = leaves(current.root);
  focusPane(l[(l.indexOf(current.focused) + step + l.length) % l.length]);
}

export function cycleTab(step) {
  if (tabs.length < 2) return;
  selectTab(tabs[(tabs.indexOf(current) + step + tabs.length) % tabs.length]);
}

export function setZoom(delta) {
  zoom = delta === 0 ? 0 : Math.max(-6, Math.min(24, zoom + delta));
  applyTerminalSettings();
}

/** Re-applies settings/theme to every live terminal. */
export function applyTerminalSettings(theme) {
  if (theme) xtheme = theme;
  const t = S.settings.terminal;
  for (const tab of tabs) for (const p of leaves(tab.root)) {
    if (!p.term) continue;
    Object.assign(p.term.options, { fontFamily: t.font_family, fontSize: t.font_size + zoom, lineHeight: t.line_height, cursorStyle: t.cursor_style, cursorBlink: t.cursor_blink, scrollback: t.scrollback, theme: xtheme });
    p.refit();
  }
}

listen("ssh-closed", (e) => bySession.get(e.payload)?.closed());
export const initPanes = render;
