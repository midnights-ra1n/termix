// Shared state + tiny DOM helpers used by every module.
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n.js";
import {
  createElement, Server, Cloud, Database, Monitor, Cpu, Container, Router, Shield, Globe, TerminalSquare,
  House, Box, HardDrive, Flame, Rocket, Folder, FolderOpen, Star, Plus, FolderPlus, Search, Settings,
  X, Upload, Download, RefreshCw, ArrowUp, File, Trash2, PanelLeftInactive, PanelRight, Columns2, Rows2,
  ChevronRight, GripVertical, Activity, FolderTree,
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
};

const OS = {
  debian: siDebian, ubuntu: siUbuntu, almalinux: siAlmalinux, rocky: siRockylinux, fedora: siFedora, arch: siArchlinux,
  alpine: siAlpinelinux, centos: siCentos, rhel: siRedhat, opensuse: siOpensuse, "opensuse-leap": siOpensuse,
  "opensuse-tumbleweed": siOpensuse, linuxmint: siLinuxmint, manjaro: siManjaro, nixos: siNixos, gentoo: siGentoo,
  kali: siKalilinux, pop: siPopos, raspbian: siRaspberrypi, proxmox: siProxmox, freebsd: siFreebsd, darwin: siApple,
};
/** Distribution logo from /etc/os-release ID (Proxmox is detected from its kernel). Returns [svg, brand colour]. */
export function osIcon(id = "", kernel = "", size = 28) {
  let key = id.toLowerCase();
  if (/-pve\b/.test(kernel)) key = "proxmox";
  else if (!key && /freebsd/i.test(kernel)) key = "freebsd";
  else if (!key && /darwin/i.test(kernel)) key = "darwin";
  const si = OS[key] || siLinux;
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
