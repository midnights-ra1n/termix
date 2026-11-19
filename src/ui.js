// Shared state + tiny DOM helpers used by every module.
import { invoke } from "@tauri-apps/api/core";
import {
  createElement, Server, Cloud, Database, Monitor, Cpu, Container, Router, Shield, Globe, TerminalSquare,
  House, Box, HardDrive, Flame, Rocket, Folder, FolderOpen, Star, Plus, FolderPlus, Search, Settings,
  X, Upload, Download, RefreshCw, ArrowUp, File, Trash2, PanelLeftInactive, PanelRight, Columns2, Rows2,
} from "lucide";

export const S = {
  settings: null,
  defaults: {},
  platform: "macos",
  version: "",
  configDir: "",
  hosts: { folders: [], hosts: [] },
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
};

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
  const t = h("div.toast." + kind, { role: kind === "error" ? "alert" : "status" }, h("span", {}, msg),
    ...actions.map(([label, fn]) => h("button.btn.small", { onclick: () => { t.remove(); fn(); } }, label)),
    h("button.icon-btn", { "aria-label": "Fermer", onclick: () => t.remove() }, icon("close", 14)));
  box.append(t);
  if (!actions.length) setTimeout(() => t.remove(), kind === "error" ? 8000 : 4000);
}

/** Modal dialog; `build(close)` returns the body. Resolves with the value passed to close(). */
export function modal(title, build, { wide = false } = {}) {
  return new Promise((resolve) => {
    const prev = document.activeElement;
    const close = (v) => { back.remove(); prev?.focus?.(); resolve(v); };
    const dlg = h("div.modal" + (wide ? ".wide" : ""), { role: "dialog", "aria-modal": "true", "aria-label": title },
      h("header", {}, h("h2", {}, title), h("button.icon-btn", { "aria-label": "Fermer", onclick: () => close() }, icon("close"))),
      build(close));
    const back = h("div.backdrop", { onmousedown: (e) => e.target === back && close(), onkeydown: (e) => e.key === "Escape" && (e.stopPropagation(), close()) }, dlg);
    document.body.append(back);
    (dlg.querySelector("[autofocus]") || dlg.querySelector("input,select,button:not(.icon-btn)"))?.focus();
  });
}

export const field = (label, input, hint) =>
  h("label.field", {}, h("span", {}, label), input, hint && h("small", {}, hint));

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
