// Right panel: server stats + SFTP browser for the focused pane's session.
import { open as openDialog, save as saveDialog, confirm } from "@tauri-apps/plugin-dialog";
import { S, h, icon, invoke, toast, fmtBytes, modal, field } from "./ui.js";
import { focused, events } from "./panes.js";

let tab = "stats";
let session = null;
let prev = null;
let timer = null;
let cwd = "";

export function initPanel() {
  const tabsEl = document.getElementById("panel-tabs");
  tabsEl.replaceChildren(...[["stats", "Stats"], ["sftp", "SFTP"]].map(([k, l]) =>
    h("button.seg" + (k === tab ? ".on" : ""), { role: "tab", "aria-selected": String(k === tab), onclick: () => { tab = k; initPanel(); refresh(); } }, l)));
  events.addEventListener("focus", refresh);
}

function refresh() {
  const p = focused();
  const s = p?.session || null;
  if (s !== session) { session = s; prev = null; cwd = ""; }
  clearInterval(timer);
  const body = document.getElementById("panel-body");
  if (!S.settings.appearance.right_panel) return;
  if (!session) return body.replaceChildren(h("p.empty", {}, "Aucune session active."));
  if (tab === "stats") { stats(); timer = setInterval(stats, 3000); }
  else sftp(cwd);
}

const bar = (label, pct, detail) => h("div.metric", {},
  h("div.metric-head", {}, h("span", {}, label), h("span.mono", {}, detail)),
  h("div.meter", { role: "meter", "aria-label": label, "aria-valuenow": Math.round(pct), "aria-valuemin": 0, "aria-valuemax": 100 },
    h("div", { style: `width:${Math.min(100, pct).toFixed(1)}%`, className: pct > 90 ? "hot" : pct > 70 ? "warm" : "" })));

async function stats() {
  const id = session;
  if (document.hidden) return;
  let st;
  try { st = await invoke("ssh_stats", { id }); } catch { return; }
  if (id !== session || tab !== "stats") return;
  const now = performance.now();
  let cpu = 0, rx = 0, tx = 0;
  if (prev) {
    const dt = st.cpu_total - prev.st.cpu_total;
    cpu = dt > 0 ? 100 * (1 - (st.cpu_idle - prev.st.cpu_idle) / dt) : 0;
    const sec = (now - prev.t) / 1000;
    rx = Math.max(0, (st.net_rx - prev.st.net_rx) / sec);
    tx = Math.max(0, (st.net_tx - prev.st.net_tx) / sec);
  }
  prev = { st, t: now };
  const body = document.getElementById("panel-body");
  if (!st.mem_total_kb) return body.replaceChildren(h("p.empty", {}, "Stats disponibles uniquement pour les serveurs Linux."));
  const memUsed = st.mem_total_kb - st.mem_avail_kb;
  body.replaceChildren(
    bar("CPU", cpu, `${cpu.toFixed(0)} %`),
    bar("Mémoire", (100 * memUsed) / st.mem_total_kb, `${fmtBytes(memUsed * 1024)} / ${fmtBytes(st.mem_total_kb * 1024)}`),
    bar("Disque /", st.disk_total_kb ? (100 * st.disk_used_kb) / st.disk_total_kb : 0, `${fmtBytes(st.disk_used_kb * 1024)} / ${fmtBytes(st.disk_total_kb * 1024)}`),
    h("div.metric", {}, h("div.metric-head", {}, h("span", {}, "Réseau"), h("span.mono", {}, `↓ ${fmtBytes(rx)}/s  ↑ ${fmtBytes(tx)}/s`))),
    h("div.metric", {}, h("div.metric-head", {}, h("span", {}, "Charge moyenne"), h("span.mono", {}, st.load.map((l) => l.toFixed(2)).join("  ")))),
  );
}

const join = (dir, name) => (dir.endsWith("/") ? dir : dir + "/") + name;

async function sftp(path) {
  const id = session;
  const body = document.getElementById("panel-body");
  let dir, entries;
  try { [dir, entries] = await invoke("sftp_list", { id, path }); }
  catch (e) { return body.replaceChildren(h("p.error", {}, "SFTP : " + e)); }
  if (id !== session || tab !== "sftp") return;
  cwd = dir;
  const up = dir.replace(/\/[^/]+\/?$/, "") || "/";
  const act = (label, ic, fn) => h("button.icon-btn", { "aria-label": label, title: label, onclick: fn }, icon(ic, 15));
  body.replaceChildren(
    h("div.sftp-bar", {},
      act("Dossier parent", "up", () => sftp(up)),
      h("input.path", { value: dir, "aria-label": "Chemin", spellcheck: false, onkeydown: (e) => e.key === "Enter" && sftp(e.target.value) }),
      act("Actualiser", "refresh", () => sftp(dir)),
      act("Envoyer un fichier", "upload", async () => {
        const files = await openDialog({ multiple: true, title: "Envoyer vers " + dir });
        for (const f of [files].flat().filter(Boolean)) {
          const name = f.split(/[\\/]/).pop();
          try { await invoke("sftp_upload", { id, local: f, remote: join(dir, name) }); toast(`${name} envoyé`, "success"); }
          catch (e) { toast(String(e), "error"); }
        }
        sftp(dir);
      }),
      act("Nouveau dossier", "folderPlus", async () => {
        const name = h("input", { autofocus: true });
        const ok = await modal("Nouveau dossier", (close) => h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field("Nom", name), h("footer", {}, h("button.btn.primary", { type: "submit" }, "Créer"))));
        if (ok && name.value) invoke("sftp_mkdir", { id, path: join(dir, name.value) }).then(() => sftp(dir), (e) => toast(String(e), "error"));
      })),
    h("ul.files", { role: "list" }, ...entries.map((e) => h("li", {},
      h("button.row", { ondblclick: () => (e.dir ? sftp(join(dir, e.name)) : download(id, dir, e)), onkeydown: (ev) => ev.key === "Enter" && (e.dir ? sftp(join(dir, e.name)) : download(id, dir, e)), title: e.name },
        icon(e.dir ? "folder" : "file", 15), h("span.name", {}, e.name), h("span.mono.mute", {}, e.dir ? "" : fmtBytes(e.size))),
      !e.dir && act("Télécharger", "download", () => download(id, dir, e)),
      act("Supprimer", "trash", async () => {
        if (!(await confirm(`Supprimer ${e.name} ?`, { kind: "warning" }))) return;
        invoke("sftp_remove", { id, path: join(dir, e.name), dir: e.dir }).then(() => sftp(dir), (er) => toast(String(er), "error"));
      })))),
  );
}

async function download(id, dir, e) {
  const local = await saveDialog({ defaultPath: e.name, title: "Télécharger " + e.name });
  if (!local) return;
  try { await invoke("sftp_download", { id, remote: join(dir, e.name), local }); toast(`${e.name} téléchargé`, "success"); }
  catch (er) { toast(String(er), "error"); }
}

export const refreshPanel = refresh;
