// Shared state + tiny DOM helpers used by every module.
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n.js";
import {
  createElement, Server, Cloud, Database, Monitor, Cpu, Container, Router, Shield, Globe, TerminalSquare,
  House, Box, HardDrive, Flame, Rocket, Folder, FolderOpen, Star, Plus, FolderPlus, Search, Settings,
  X, Upload, Download, RefreshCw, ArrowUp, File, Trash2, PanelLeftInactive, PanelRight, Columns2, Rows2,
  ChevronRight, GripVertical, Activity, FolderTree, CloudCheck, CloudOff, CloudAlert, CloudSync,
  Plug, Layers, Pencil, Copy, StarOff, ClipboardPaste, Scissors, Eraser, SquareDashed,
  Eye, EyeOff, FilePlus, Ellipsis, Lock, Tag, Tags, Link, SquareTerminal, RotateCw,
} from "lucide";
import {
  siDebian, siUbuntu, siAlmalinux, siRockylinux, siFedora, siArchlinux, siAlpinelinux, siCentos, siRedhat, siOpensuse,
  siProxmox, siLinux, siFreebsd, siApple, siRaspberrypi, siLinuxmint, siManjaro, siNixos, siGentoo, siKalilinux, siPopos,
} from "simple-icons";

export const S = {
  settings: null,
  defaults: {},
  platform: "macos",
  version: "",
  configDir: "",
  hosts: { folders: [], hosts: [] },
  status: {},
};
export const isMac = () => S.platform === "macos";
export { invoke };

/** Icons offered for hosts (built-in set). */
export const HOST_ICONS = {
  server: Server, cloud: Cloud, database: Database, desktop: Monitor, board: Cpu, docker: Container,
  router: Router, firewall: Shield, web: Globe, terminal: TerminalSquare, home: House, box: Box,
  nas: HardDrive, prod: Flame, deploy: Rocket,
};
const UI_ICONS = {
  folder: Folder, folderOpen: FolderOpen, star: Star, plus: Plus, folderPlus: FolderPlus, search: Search,
  settings: Settings, close: X, upload: Upload, download: Download, refresh: RefreshCw, up: ArrowUp,
  file: File, trash: Trash2, sidebar: PanelLeftInactive, panel: PanelRight, splitH: Columns2, splitV: Rows2,
  chevron: ChevronRight, grip: GripVertical, stats: Activity, folderTree: FolderTree,
  syncOk: CloudCheck, syncOff: CloudOff, syncConflict: CloudAlert, syncing: CloudSync,
  connect: Plug, tmux: Layers, edit: Pencil, copy: Copy, unstar: StarOff, paste: ClipboardPaste, cut: Scissors,
  clear: Eraser, selectAll: SquareDashed, eye: Eye, eyeOff: EyeOff, filePlus: FilePlus, more: Ellipsis, lock: Lock,
  tag: Tag, tags: Tags, link: Link, cdHere: SquareTerminal, reconnect: RotateCw,
};

const OS = {
  debian: siDebian, ubuntu: siUbuntu, almalinux: siAlmalinux, rocky: siRockylinux, fedora: siFedora, arch: siArchlinux,
  alpine: siAlpinelinux, centos: siCentos, rhel: siRedhat, opensuse: siOpensuse, "opensuse-leap": siOpensuse,
  "opensuse-tumbleweed": siOpensuse, linuxmint: siLinuxmint, manjaro: siManjaro, nixos: siNixos, gentoo: siGentoo,
  kali: siKalilinux, pop: siPopos, raspbian: siRaspberrypi, proxmox: siProxmox, freebsd: siFreebsd, darwin: siApple, linux: siLinux,
};
/** Logos offered in the host editor (one per distribution: aliases left out). */
export const OS_LOGOS = Object.entries(OS).filter(([k, si], i, all) => all.findIndex(([, x]) => x === si) === i).map(([k, si]) => [k, si.title]);
/** Key into OS from /etc/os-release ID and kernel (Proxmox is detected from its kernel); "" with nothing to go on. */
export function osKey(id = "", kernel = "") {
  const key = id.toLowerCase();
  if (/-pve\b/.test(kernel)) return "proxmox";
  if (key) return OS[key] ? key : "linux";
  if (/freebsd/i.test(kernel)) return "freebsd";
  if (/darwin/i.test(kernel)) return "darwin";
  return kernel ? "linux" : "";
}
/** Distribution logo. Returns [svg, brand colour]. */
export function osIcon(id = "", kernel = "", size = 28) {
  const si = OS[osKey(id, kernel)] || siLinux;
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", size);
  svg.setAttribute("height", size);
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", si.path);
  path.setAttribute("fill", "currentColor");
  svg.append(path);
  return [svg, "#" + si.hex];
}

export function icon(name, size = 16) {
  const node = HOST_ICONS[name] || UI_ICONS[name] || Server;
  const el = createElement(node);
  el.setAttribute("width", size);
  el.setAttribute("height", size);
  el.setAttribute("aria-hidden", "true");
  return el;
}

/** A host's icon: built-in set, a distribution logo ("os:debian"), or the detected one ("os:auto", see panel.js). */
export function hostIcon(x, size = 15) {
  const ic = x.icon || "server";
  if (!ic.startsWith("os:")) return icon(ic, size);
  const key = ic === "os:auto" ? x.os : ic.slice(3);
  return key ? osIcon(key, "", size)[0] : icon("server", size);
}

/** Stable hue per tag name, so a tag keeps its colour everywhere. */
export const tagHue = (tag) => [...tag].reduce((n, c) => (n * 31 + c.charCodeAt(0)) >>> 0, 7) % 360;

/** h("div.cls", {attrs/on*}, ...children) */
export function h(tag, props = {}, ...kids) {
  const [name, ...cls] = tag.split(".");
  const el = document.createElement(name || "div");
  if (cls.length) el.className = cls.join(" ");
  if (name === "input" || name === "textarea") {
    // No spellcheck/autocorrect on hostnames, paths and commands (macOS WebKit enables them by default).
    el.spellcheck = false;
    el.setAttribute("autocorrect", "off");
    el.setAttribute("autocapitalize", "off");
    el.setAttribute("autocomplete", "off");
  }
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k === "dataset") Object.assign(el.dataset, v);
    else if (k in el && k !== "list" && k !== "type") el[k] = v;
    else el.setAttribute(k, v === true ? "" : v);
  }
  for (const k of kids.flat()) if (k != null && k !== false) el.append(k);
  return el;
}

export const uid = () => crypto.randomUUID();

export function toast(msg, kind = "info", actions = []) {
  const box = document.getElementById("toasts");
  const el = h("div.toast." + kind, { role: kind === "error" ? "alert" : "status" }, h("span", {}, msg),
    ...actions.map(([label, fn]) => h("button.btn.small", { onclick: () => { el.remove(); fn(); } }, label)),
    h("button.icon-btn", { "aria-label": t("close"), onclick: () => el.remove() }, icon("close", 14)));
  box.append(el);
  if (!actions.length) setTimeout(() => el.remove(), kind === "error" ? 8000 : 4000);
}

/** Modal dialog; `build(close)` returns the body. Resolves with the value passed to close(). */
export function modal(title, build, { wide = false, cls = "" } = {}) {
  return new Promise((resolve) => {
    const prev = document.activeElement;
    const close = (v) => { back.remove(); prev?.focus?.(); resolve(v); };
    const dlg = h("div.modal" + (wide ? ".wide" : "") + (cls ? "." + cls : ""), { role: "dialog", "aria-modal": "true", "aria-label": title },
      h("header", {}, h("h2", {}, title), h("button.icon-btn", { "aria-label": t("close"), onclick: () => close() }, icon("close"))),
      build(close));
    const back = h("div.backdrop", { onmousedown: (e) => e.target === back && close(), onkeydown: (e) => e.key === "Escape" && (e.stopPropagation(), close()) }, dlg);
    document.body.append(back);
    (dlg.querySelector("[autofocus]") || dlg.querySelector("input,select,button:not(.icon-btn)"))?.focus();
  });
}

export const field = (label, input, hint) =>
  h("label.field", {}, h("span", {}, label), input, hint && h("small", {}, hint));

export function fmtUptime(s) {
  const d = Math.floor(s / 86400), hh = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return d ? `${d} d ${hh} h` : hh ? `${hh} h ${m} min` : `${m} min`;
}

/** Status dot for a host: up (green), ping (yellow), down (red), unknown (grey). */
export const statusDot = (id) => {
  const st = S.status[id] || "unknown";
  return h("span.status." + st, { role: "img", "aria-label": t("st_" + st), title: t("st_" + st) });
};

export function fmtBytes(n) {
  const u = ["o", "Ko", "Mo", "Go", "To"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
}

export async function saveHosts() {
  await invoke("save_hosts", { doc: S.hosts });
  document.dispatchEvent(new Event("hosts-changed"));
}

/**
 * Context menu drawn in the page: native popups open as a separate, centred window on Wayland / Windows.
 * items: { label, icon, action, disabled, danger } or "-" (separator).
 */
export function contextMenu(e, items) {
  document.querySelector(".ctx-menu")?.remove();
  const prev = document.activeElement;
  const done = () => { menu.remove(); removeEventListener("mousedown", outside, true); removeEventListener("keydown", key, true); removeEventListener("blur", done); removeEventListener("resize", done); };
  const outside = (ev) => !menu.contains(ev.target) && done();
  const menu = h("div.mb-drop.ctx-menu", { role: "menu", onmousedown: (ev) => ev.preventDefault() }, // keeps the focus (and text selection) where it was
    ...items.filter(Boolean).map((x) => x === "-" ? h("div.mb-sep", { role: "separator" })
      : h("button.mb-item" + (x.danger ? ".danger" : ""), { type: "button", role: "menuitem", disabled: x.disabled,
          onclick: () => { done(); prev?.focus?.(); x.action(); } },
          h("span.ctx-label", {}, icon(x.icon, 15), x.label))));
  // Like a native menu: Escape and arrows go to it even while the focus stays in the terminal.
  const key = (ev) => {
    const list = [...menu.querySelectorAll("button:not(:disabled)")], at = list.indexOf(document.activeElement);
    const step = { ArrowDown: 1, ArrowUp: -1 }[ev.key];
    if (ev.key === "Escape") { done(); prev?.focus?.(); }
    else if (step) list[at < 0 ? (step > 0 ? 0 : list.length - 1) : (at + step + list.length) % list.length]?.focus();
    else return;
    ev.preventDefault(); ev.stopPropagation();
  };
  document.body.append(menu);
  // Keyboard-opened menus (Shift+F10, menu key) have no pointer position: anchor on the element.
  const r = e.target.getBoundingClientRect?.(), keyb = !e.clientX && !e.clientY && r;
  const x = keyb ? r.left : e.clientX, y = keyb ? r.bottom : e.clientY;
  menu.style.left = Math.max(4, Math.min(x, innerWidth - menu.offsetWidth - 4)) + "px";
  menu.style.top = Math.max(4, y + menu.offsetHeight > innerHeight - 4 ? y - menu.offsetHeight : y) + "px";
  if (keyb) menu.querySelector("button:not(:disabled)")?.focus();
  addEventListener("mousedown", outside, true);
  addEventListener("keydown", key, true);
  addEventListener("blur", done);
  addEventListener("resize", done);
}
