// Tabs, split panes (tmux-like) and xterm terminals bound to SSH sessions.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { S, h, icon, invoke, modal, field, toast, statusDot, uid, saveHosts, contextMenu } from "./ui.js";
import { t as tr } from "./i18n.js";
import { refreshChrome } from "./native.js";
import { Suggest } from "./suggest.js";
import logo from "../assets/icon-rounded.svg";

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
    this.el.addEventListener("contextmenu", (e) => { if (e.target.closest("input")) return; e.preventDefault(); this.contextMenu(e); });
    // Drop a host from the sidebar onto a pane: connect there (or in a new tab if it is busy).
    this.el.addEventListener("dragover", (e) => { if (e.dataTransfer.types.includes("text/plain")) { e.preventDefault(); this.el.classList.add("drop"); } });
    this.el.addEventListener("dragleave", (e) => { if (!this.el.contains(e.relatedTarget)) this.el.classList.remove("drop"); });
    this.el.addEventListener("drop", (e) => {
      this.el.classList.remove("drop");
      const [kind, id] = e.dataTransfer.getData("text/plain").split(":");
      const host = kind === "host" && S.hosts.hosts.find((x) => x.id === id);
      if (!host) return;
      e.preventDefault();
      if (this.term) newTab(host); else { focusPane(this); this.connect(host); }
    });
    this.session = null;
    this.host = null;
    this.term = null;
    this.picker();
  }
  picker() {
    this.tmux = undefined; // a new host gets its own tmux session
    const list = h("div.picker-list");
    const input = h("input", { placeholder: tr("search_host"), "aria-label": tr("search_host") });
    const draw = () => {
      const q = input.value.toLowerCase();
      list.replaceChildren(...S.hosts.hosts.filter((x) => !q || (x.name + x.host + x.tags.join(" ")).toLowerCase().includes(q)).slice(0, 30)
        .map((x) => h("button.row.host", { onclick: () => this.connect(x) }, statusDot(x.id), icon(x.icon || "server", 15), h("span.name", {}, x.name), h("span.mute.mono", {}, x.host))));
    };
    input.oninput = draw;
    input.onkeydown = (e) => { if (e.key === "Enter") list.querySelector("button")?.click(); };
    draw();
    this.el.replaceChildren(h("div.picker", {}, h("h3", {}, tr("open_conn")), input, list,
      h("button.btn.small", { type: "button", onclick: () => quickConnect((x) => this.connect(x)) }, tr("quick_connect"))));
    this.focusTarget = input;
  }
  async connect(host, password = null) {
    this.host = host;
    // Kept across reconnects (Enter after a drop) so the same tmux session is reattached.
    if (this.tmux === undefined) this.tmux = wantsTmux(host) ? tmuxName() : null;
    this.el.replaceChildren(h("div.connecting", { role: "status" }, h("span.dot"), tr("connecting", host.name)));
    const term = new Terminal({
      ...termOptions(), allowProposedApi: true, macOptionIsMeta: true, rightClickSelectsWord: true,
      // ⌥-drag (Shift-drag elsewhere) selects text even when tmux has the mouse.
      macOptionClickForcesSelection: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const ch = new Channel();
    ch.onmessage = (buf) => term.write(new Uint8Array(buf));
    try {
      this.session = await invoke("ssh_connect", { hostId: host.id, host: host.id.startsWith("quick:") ? host : null, cols: 80, rows: 24, password, tmux: this.tmux, onData: ch });
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
    if (S.settings.performance.gpu) this.gpu();
    // Shells and tmux send DECRST 12 (terminfo cnorm), which would stop the blink for good: the setting wins.
    for (const final of ["h", "l"]) term.parser.registerCsiHandler({ prefix: "?", final }, (p) => p.length === 1 && p[0] === 12);
    term.onData((d) => invoke("ssh_write", { id: this.session, data: withCtrl(d) }).catch(() => {}));
    term.onResize(({ cols, rows }) => invoke("ssh_resize", { id: this.session, cols, rows }).catch(() => {}));
    term.onSelectionChange(() => S.settings.terminal.copy_on_select && term.hasSelection() && navigator.clipboard.writeText(term.getSelection()));
    const id = this.session;
    this.suggest = new Suggest(term, host.id, (d) => invoke("ssh_write", { id, data: d }).catch(() => {}), () => S.settings.terminal.suggestions);
    invoke("ssh_history", { id }).then((list) => this.suggest?.setRemote(list), () => {});
    term.attachCustomKeyEventHandler((e) => {
      if (!this.suggest.handleKey(e)) return false;
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
    const ok = await modal(tr("pw_for", `${host.user || ""}@${host.host}`), (close) =>
      h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field(tr("f_password"), pw),
        h("label.check", {}, remember, tr("remember")),
        h("footer", {}, h("button.btn", { type: "button", onclick: () => close(false) }, tr("cancel")), h("button.btn.primary", { type: "submit" }, tr("connect")))));
    if (!ok) return this.picker();
    if (remember.checked) await invoke("set_secret", { id: host.id, value: pw.value }).catch((e) => toast(String(e), "error"));
    this.connect(host, pw.value);
  }
  failed(host, msg) {
    this.el.replaceChildren(h("div.picker", {}, h("h3.error", {}, tr("failed", host.name)), h("p.mute", {}, msg),
      h("div.inline", {}, h("button.btn.primary", { onclick: () => this.connect(host) }, tr("retry")), h("button.btn", { onclick: () => this.picker() }, tr("other_host")))));
  }
  closed() {
    bySession.delete(this.session);
    this.session = null;
    this.term?.write(`\r\n\x1b[2m${tr("conn_closed")}\x1b[0m\r\n`);
    const t = this.term;
    const sub = t?.onKey(({ domEvent }) => { if (domEvent.key === "Enter") { sub.dispose(); this.dispose(); this.connect(this.host); } });
    events.dispatchEvent(new Event("focus"));
  }
  contextMenu(e) {
    const term = this.term;
    focusPane(this);
    contextMenu(e, [
      ...(term ? [
        { label: tr("ctx_copy"), icon: "copy", disabled: !term.hasSelection(), action: () => navigator.clipboard.writeText(term.getSelection()) },
        { label: tr("ctx_paste"), icon: "paste", action: async () => term.paste(await navigator.clipboard.readText()) },
        { label: tr("ctx_select_all"), icon: "selectAll", action: () => term.selectAll() },
        { label: tr("ctx_clear"), icon: "clear", action: () => this.clearScreen() },
        "-",
      ] : []),
      { label: tr("split_right"), icon: "splitH", action: () => split("right") },
      { label: tr("split_down"), icon: "splitV", action: () => split("down") },
      "-",
      { label: tr("close_pane"), icon: "close", action: () => closePane() },
    ]);
  }
  refit() {
    if (!this.fit || !this.el.isConnected) return;
    try { this.fit.fit(); } catch {}
  }
  /** Like typing `clear`, without touching what is being typed: local scrollback wiped, then Ctrl+L
   *  so the shell redraws its prompt (a full-screen program just repaints). */
  clearScreen() {
    if (!this.term) return;
    this.term.clear();
    if (this.session) invoke("ssh_write", { id: this.session, data: "\x0c" }).catch(() => {});
  }
  /** GPU renderer: far smoother on heavy output. Without WebGL2, or when the browser drops the context
   *  (too many live terminals), xterm falls back to its DOM renderer. */
  gpu() {
    try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => { gl.dispose(); if (this.webgl === gl) this.webgl = null; });
      this.term.loadAddon(gl);
      this.webgl = gl;
    } catch { this.webgl = null; }
  }
  dispose() {
    if (this.session) invoke("ssh_close", { id: this.session });
    bySession.delete(this.session);
    this.ro?.disconnect();
    this.suggest?.dispose();
    this.suggest = null;
    this.term?.dispose(); // also disposes its addons (WebGL context included)
    this.term = this.fit = this.webgl = null;
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
    h("button.icon-btn", { "aria-label": `${tr("close_tab")} ${i + 1}`, onclick: () => closeTab(t) }, icon("close", 12)))),
    h("button.icon-btn.new-tab", { "aria-label": tr("new_tab"), title: tr("new_tab"), onclick: () => newTab() }, icon("plus", 14)));
  const area = document.getElementById("panes");
  area.replaceChildren(current ? renderNode(current.root) : welcome());
  current && leaves(current.root).forEach((p) => p.el.classList.toggle("focused", p === current.focused));
  saveWorkspace();
  refreshChrome();
}

/** iOS title bar (main.js): the tabs, and switching or closing one by its position. */
export const tabList = () => tabs.map((t) => ({ l: t.title(), on: t === current }));
export const selectTabAt = (i) => tabs[i] && selectTab(tabs[i]);
export const closeTabAt = (i) => tabs[i] && closeTab(tabs[i]);

// ---------- workspace: tabs, splits and tmux sessions, synced so another device can reopen them ----------

function serialize(n) {
  if (n instanceof Pane) return n.host ? { host: n.host.id, tmux: n.tmux || null } : null;
  const a = serialize(n.a), b = serialize(n.b);
  return a && b ? { split: n.split, ratio: n.ratio, a, b } : a || b;
}
let wsTimer;
function saveWorkspace() {
  clearTimeout(wsTimer);
  wsTimer = setTimeout(() => invoke("sync_set_workspace", { tabs: tabs.map((t) => serialize(t.root)).filter(Boolean) }).catch(() => {}), 2000);
}

/** Reopens another device's tabs and splits, reattaching each pane's tmux session. */
export function openWorkspace(ws) {
  for (const node of ws.tabs) {
    const build = (n) => {
      if (n.split) return { split: n.split, ratio: n.ratio, a: build(n.a), b: build(n.b) };
      const p = new Pane();
      p.tmux = n.tmux || undefined;
      p.pending = S.hosts.hosts.find((x) => x.id === n.host);
      return p;
    };
    const root = build(node);
    const tab = addTab(root);
    selectTab(tab);
    for (const p of leaves(root)) if (p.pending) { p.connect(p.pending); delete p.pending; }
  }
}

function updateTabTitles() { render(); }

/** An unsaved host from "user@host:port" (user and port optional), or null. */
export function quickHost(spec) {
  const m = spec.trim().match(/^(?:([^@\s]+)@)?([^\s@:]+|\[[^\]\s]+\])(?::(\d{1,5}))?$/);
  const port = m && Number(m[3] || 22);
  if (!m || port < 1 || port > 65535) return null;
  const host = m[2].replace(/^\[|\]$/g, ""), user = m[1] || "";
  const name = `${user ? user + "@" : ""}${m[2]}${port !== 22 ? ":" + port : ""}`;
  return { id: "quick:" + name, name, host, port, user, key_path: "", key_id: "", description: "", icon: "terminal", tags: [], folder: null, favorite: false, tmux: "" };
}

/** Connect to user@host:port without saving it first (or saving it on the way). */
export async function quickConnect(open = (x) => openHost(x)) {
  const input = h("input", { placeholder: "user@example.com:22", autofocus: true, "aria-label": tr("quick_connect") });
  const save = h("input", { type: "checkbox", className: "switch" });
  const err = h("p.error.small", { role: "alert" });
  const host = await modal(tr("quick_connect"), (close) => h("form.grid", { onsubmit: (e) => {
      e.preventDefault();
      const x = quickHost(input.value);
      if (!x) return (err.textContent = tr("quick_bad"));
      close(x);
    } },
    h("div.span2", {}, field(tr("quick_target"), input, tr("quick_hint"))),
    h("label.toggle.span2", {}, h("span", {}, tr("quick_save")), save), h("div.span2", {}, err),
    h("footer.span2", {}, h("button.btn", { type: "button", onclick: () => close(null) }, tr("cancel")), h("button.btn.primary", { type: "submit" }, tr("connect")))));
  if (!host) return;
  if (save.checked) {
    Object.assign(host, { id: uid(), name: host.host, icon: "server" });
    S.hosts.hosts.push(host);
    await saveHosts().catch((e) => toast(String(e), "error"));
  }
  open(host);
}

function welcome() {
  const resume = h("div.resume");
  const recent = h("ul.recent", { "aria-label": tr("recent_sessions") });
  const el = h("div.welcome", {}, h("h1", {}, h("img", { src: logo, alt: "" }), "Termix"), h("p", {}, tr("welcome")),
    h("div.resume", {}, h("button.btn.primary", { onclick: () => quickConnect() }, icon("terminal", 14), tr("quick_connect"))), recent, resume);
  // The last 5 sessions: saved hosts that still exist, and quick connects.
  invoke("recent_sessions").then((ids) => {
    const list = ids.map((id) => (id.startsWith("quick:") ? quickHost(id.slice(6)) : S.hosts.hosts.find((x) => x.id === id))).filter(Boolean).slice(0, 5);
    if (list.length) recent.replaceChildren(h("li.recent-title", {}, tr("recent_sessions")), ...list.map((x) => h("li", {},
      h("button.row.host", { onclick: () => newTab(x) }, statusDot(x.id), icon(x.icon || "server", 15), h("span.name", {}, x.name),
        h("span.mute.mono", {}, `${x.user ? x.user + "@" : ""}${x.host}`)))));
  }).catch(() => {});
  invoke("sync_workspaces").then((list) => resume.replaceChildren(...list.slice(0, 3).map((ws) =>
    h("button.btn", { onclick: () => openWorkspace(ws) }, icon("terminal", 14), tr("resume_ws", ws.name, ws.tabs.length))))).catch(() => {});
  el.addEventListener("dragover", (e) => e.dataTransfer.types.includes("text/plain") && e.preventDefault());
  el.addEventListener("drop", (e) => {
    const [kind, id] = e.dataTransfer.getData("text/plain").split(":");
    const host = kind === "host" && S.hosts.hosts.find((x) => x.id === id);
    if (host) { e.preventDefault(); newTab(host); }
  });
  return el;
}

function addTab(root) {
  const tab = { root, focused: leaves(root)[0], title() { const l = leaves(this.root); const n = l[0].host?.name || tr("new_tab"); return l.length > 1 ? `${n} +${l.length - 1}` : n; } };
  tabs.push(tab);
  return tab;
}

export function newTab(host = null, tmux = undefined) {
  const p = new Pane();
  p.tmux = tmux;
  selectTab(addTab(p));
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
/** Gives the keyboard back to the focused pane (terminal, or its host picker). */
export const focusTerminal = () => current?.focused?.focusTarget?.focus();

/** The host's own tmux choice, else the global setting. */
export const wantsTmux = (host) => host.tmux === "on" || (host.tmux !== "off" && S.settings.terminal.tmux);
const tmuxName = () => "tx-" + crypto.randomUUID().slice(0, 8);

/** Open a host: in the focused pane if it's empty, else a new tab. `opts.tmux` (bool) overrides the host's choice once. */
export function openHost(host, opts) {
  const tmux = typeof opts?.tmux === "boolean" ? (opts.tmux ? tmuxName() : null) : undefined;
  const p = focused();
  if (p && !p.session && !p.term) { p.tmux = tmux; return p.connect(host); }
  newTab(host, tmux);
}

/** Session to show in the side panel: focused pane first, else any live pane of the tab. */
export function activeSession() {
  if (!current) return null;
  if (current.focused?.session) return current.focused;
  return leaves(current.root).find((p) => p.session) || null;
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

/** Background image or see-through colour: xterm draws no background, the pane's CSS layers do. */
/** Below 100 % the pane's CSS layers (image, colour) show through xterm. At 100 % they would be hidden
 *  anyway: xterm stays opaque, which keeps subpixel (ClearType) text on Windows. */
const customBg = () => S.settings.terminal.background_opacity < 100;
/** Same colour, alpha 0: xterm draws no background, but inverse video (htop, tmux bar, less) and contrast
 *  checks still use the real colour instead of black. */
const clearBg = (c) => (/^#[0-9a-f]{6}$/i.test(c) ? c + "00" : "#00000000");

function termOptions() {
  const t = S.settings.terminal, see = customBg();
  return {
    fontFamily: t.font_family, fontSize: t.font_size + zoom, lineHeight: t.line_height, cursorStyle: t.cursor_style,
    cursorBlink: t.cursor_blink, scrollback: t.scrollback, fontWeight: t.font_weight,
    allowTransparency: see, theme: see ? { ...xtheme, background: clearBg(xtheme.background) } : xtheme,
    letterSpacing: t.letter_spacing, drawBoldTextInBrightColors: t.bold_is_bright,
  };
}

/** Re-applies settings/theme to every live terminal. */
export function applyTerminalSettings(theme) {
  if (theme) xtheme = theme;
  document.documentElement.style.setProperty("--term-pad", S.settings.terminal.padding + "px");
  document.documentElement.style.setProperty("--focus-w", Math.min(6, Math.max(1, S.settings.terminal.focus_width || 2)) + "px");
  document.body.classList.toggle("focus-single", S.settings.terminal.focus_single);
  document.body.classList.toggle("no-anim", !S.settings.performance.animations);
  applyBackground();
  // xterm measures the cell once per font change: wait until a bundled font is actually loaded.
  const { font_family: ff, font_size: fs } = S.settings.terminal;
  Promise.all([`${fs}px ${ff}`, `bold ${fs}px ${ff}`].map((f) => document.fonts.load(f))).catch(() => {}).then(() => {
    const opts = termOptions(), gpu = S.settings.performance.gpu;
    for (const tab of tabs) for (const p of leaves(tab.root)) {
      if (!p.term) continue;
      // The WebGL renderer picks transparency up at creation: rebuild it when that changes.
      if (p.webgl && (!gpu || p.term.options.allowTransparency !== opts.allowTransparency)) { p.webgl.dispose(); p.webgl = null; }
      Object.assign(p.term.options, opts);
      if (gpu && !p.webgl) p.gpu();
      p.webgl?.clearTextureAtlas(); // glyphs drawn with the fallback font before it loaded
      p.refit();
    }
  });
}

const FIT = { cover: ["cover", "no-repeat"], contain: ["contain", "no-repeat"], tile: ["auto", "repeat"], center: ["auto", "no-repeat"], stretch: ["100% 100%", "no-repeat"] };
let bgPath = "";
const bgWarned = new Set();
/** Terminal background layers (style.css, body.see-through): image under a colour of adjustable opacity. */
function applyBackground() {
  const t = S.settings.terminal, r = document.documentElement.style;
  const [size, repeat] = FIT[t.background_image_fit] || FIT.cover;
  document.body.classList.toggle("see-through", customBg());
  r.setProperty("--term-op", Math.min(100, t.background_opacity) + "%");
  r.setProperty("--term-blur", t.background_blur + "px");
  r.setProperty("--term-img-op", t.background_image_opacity / 100);
  r.setProperty("--term-img-size", size);
  r.setProperty("--term-img-repeat", repeat);
  r.setProperty("--term-img-pos", `${t.background_image_x || "center"} ${t.background_image_y || "center"}`);
  if (t.background_image === bgPath) return;
  const want = bgPath = t.background_image;
  if (!want) return r.removeProperty("--term-img");
  invoke("read_image", { path: want }).then((url) => bgPath === want && r.setProperty("--term-img", `url("${url}")`), (e) => {
    r.removeProperty("--term-img");
    bgPath = ""; // retried on the next settings change (e.g. when the sync delivers the file)
    if (!bgWarned.has(want)) { bgWarned.add(want); toast(String(e), "error"); }
  });
}

/** Clears the focused terminal (menu, shortcut, title bar button). */
export const clearScreen = () => current?.focused?.clearScreen();

listen("ssh-closed", (e) => bySession.get(e.payload)?.closed());
export const initPanes = render;

// ---------- iOS: the keys an on-screen keyboard lacks, in the bar above it (native-ui KeyBar) ----------

let ctrl = false;
export const ctrlOn = () => ctrl;
/** Ctrl from the key bar applies to the next key typed: Ctrl+letter (and @ [ \ ] ^ _) is its control code. */
function withCtrl(d) {
  if (!ctrl || d.length !== 1) return d;
  ctrl = false;
  refreshChrome();
  const c = d.toUpperCase().charCodeAt(0);
  return c >= 64 && c <= 95 ? String.fromCharCode(c - 64) : d;
}

export function keyInput(key, text) {
  if (key === "ctrl") { ctrl = !ctrl; return refreshChrome(); }
  if (key === "hide") return document.activeElement?.blur();
  const term = current?.focused?.term;
  if (!term) return;
  // vim, less… switch the arrows to "application" mode (ESC O A instead of ESC [ A).
  const arrow = { left: "D", up: "A", down: "B", right: "C" }[key];
  if (arrow) term.input((term.modes.applicationCursorKeysMode ? "\x1bO" : "\x1b[") + arrow);
  else if (key === "paste") term.paste(text || "");
  else if (key === "esc") term.input("\x1b");
  else if (key === "tab") term.input("\t");
}
