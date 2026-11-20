// Windows/Linux title bar: the same menus as the macOS menu bar (one model in Rust, `get_menu`),
// then Windows-style minimize / maximize / close buttons. macOS keeps its native menu bar.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { h, invoke } from "./ui.js";
import { t } from "./i18n.js";
import { focused } from "./panes.js";
import appIcon from "../src-tauri/icons/32x32.png";

const win = getCurrentWindow();
// Focused terminal, if the keyboard is in one (otherwise edit commands act on the focused field).
const term = () => (document.activeElement?.closest(".xterm") ? focused()?.term : null);
const exec = (cmd) => () => document.execCommand(cmd);
const toggleFullscreen = async () => win.setFullscreen(!(await win.isFullscreen()));
/** "@x" items: [shortcut shown, action]. Copy/paste use Ctrl+Shift like the terminal (Ctrl+C is SIGINT there). */
const NATIVE = {
  undo: ["Ctrl+Z", exec("undo")],
  redo: ["Ctrl+Y", exec("redo")],
  cut: ["Ctrl+X", exec("cut")],
  copy: ["Ctrl+Shift+C", () => { const x = term(); x ? navigator.clipboard.writeText(x.getSelection()) : document.execCommand("copy"); }],
  paste: ["Ctrl+Shift+V", async () => { const x = term(), s = await navigator.clipboard.readText(); x ? x.paste(s) : document.execCommand("insertText", false, s); }],
  select_all: ["Ctrl+A", () => { const x = term(); x ? x.selectAll() : document.execCommand("selectAll"); }],
  fullscreen: ["F11", toggleFullscreen],
  minimize: ["", () => win.minimize()],
  maximize: ["", () => win.toggleMaximize()],
  close_window: ["Alt+F4", () => win.close()],
  quit_app: ["", () => invoke("menu_click", { id: "quit" })],
};

const SVG = {
  min: '<path d="M0 5.5h10"/>',
  max: '<path d="M.5.5h9v9h-9z"/>',
  restore: '<path d="M.5 2.5h7v7h-7z M2.5 2.5V.5h7v7h-2"/>',
  close: '<path d="M.5.5l9 9M9.5.5l-9 9"/>',
};
const glyph = (name) => {
  const el = h("span." + name);
  el.innerHTML = `<svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" aria-hidden="true">${SVG[name]}</svg>`;
  return el;
};

let bar, menus, model = [], openIdx = -1, prevFocus = null, seq = 0;
const titles = () => [...menus.children];
const items = () => [...bar.querySelectorAll(".mb-item")];

export function initMenubar() {
  if (document.body.dataset.platform === "macos") return;
  const keep = { onmousedown: (e) => e.preventDefault() }; // clicking a menu leaves the focus in the terminal
  menus = h("div.mb-menus", { ...keep, role: "menubar" });
  const max = h("button", { ...keep, type: "button", onclick: () => win.toggleMaximize() }, glyph("max"), glyph("restore"));
  bar = h("nav", { id: "menubar", onkeydown: onKey },
    h("img.mb-icon", { src: appIcon, alt: "", "data-tauri-drag-region": true }),
    menus,
    h("div.tb-spacer", { "data-tauri-drag-region": true }),
    h("div.wc", {},
      h("button", { ...keep, type: "button", "aria-label": t("minimize"), onclick: () => win.minimize() }, glyph("min")),
      max,
      h("button.close", { ...keep, type: "button", "aria-label": t("close_window"), onclick: () => win.close() }, glyph("close"))));
  document.getElementById("app").prepend(bar);
  document.body.classList.add("menubar");
  // Square corners and the "restore" glyph while maximized / full screen.
  const sync = async () => {
    const m = (await win.isMaximized()) || (await win.isFullscreen());
    document.body.classList.toggle("maximized", m);
    max.setAttribute("aria-label", t(m ? "restore" : "maximize"));
  };
  win.onResized(sync);
  sync();
  addEventListener("mousedown", (e) => openIdx >= 0 && !bar.contains(e.target) && close());
  addEventListener("blur", close);
  addEventListener("keydown", (e) => { if (e.key === "F11") { e.preventDefault(); toggleFullscreen(); } });
  refreshMenubar();
}

/** Re-reads titles after a language change. */
export async function refreshMenubar() {
  if (!bar) return;
  model = await invoke("get_menu");
  menus.replaceChildren(...model.map(([, title], i) => h("button.mb-title", {
    type: "button", role: "menuitem", "aria-haspopup": "menu", "aria-expanded": "false",
    onclick: () => (openIdx === i ? close() : open(i)),
    onmouseenter: () => openIdx >= 0 && openIdx !== i && open(i),
  }, title)));
}

async function open(i, focusFirst = false) {
  if (openIdx < 0) prevFocus = document.activeElement;
  const n = ++seq;
  model = await invoke("get_menu"); // fresh hosts, labels and shortcuts every time
  if (n !== seq) return;
  i = (i + model.length) % model.length;
  closeDrop();
  openIdx = i;
  const btn = titles()[i];
  btn.classList.add("on");
  btn.setAttribute("aria-expanded", "true");
  const drop = h("div.mb-drop", { role: "menu", onmousedown: (e) => e.preventDefault() }, ...model[i][2].map((x) => x.id === "-"
    ? h("div.mb-sep", { role: "separator" })
    : h("button.mb-item", { type: "button", role: "menuitem", onclick: () => run(x.id) },
      h("span", {}, x.label), h("span.mb-key", {}, x.id.startsWith("@") ? NATIVE[x.id.slice(1)]?.[0] : x.accel.replace("CmdOrCtrl", "Ctrl")))));
  drop.style.left = btn.offsetLeft + "px";
  bar.append(drop);
  if (focusFirst) drop.querySelector("button")?.focus();
}

function closeDrop() {
  bar.querySelector(".mb-drop")?.remove();
  for (const b of titles()) { b.classList.remove("on"); b.setAttribute("aria-expanded", "false"); }
}

function close() {
  if (openIdx < 0) return;
  seq++;
  openIdx = -1;
  closeDrop();
}

function run(id) {
  close();
  prevFocus?.focus?.();
  if (id.startsWith("@")) NATIVE[id.slice(1)]?.[1]();
  else invoke("menu_click", { id });
}

function onKey(e) {
  const list = items(), at = list.indexOf(document.activeElement), ti = titles().indexOf(document.activeElement);
  const move = { ArrowDown: 1, ArrowUp: -1 }[e.key], side = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
  if (e.key === "Escape" && openIdx >= 0) { const i = openIdx; close(); titles()[i].focus(); }
  else if (move && at >= 0) list[(at + move + list.length) % list.length].focus();
  else if (move && ti >= 0) open(ti, true);
  else if (side && openIdx >= 0) open(openIdx + side, true);
  else if (side && ti >= 0) titles()[(ti + side + model.length) % model.length].focus();
  else return;
  e.preventDefault();
}
