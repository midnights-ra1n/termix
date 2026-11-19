// Right panel: server stats + SFTP browser for the active session.
import { open as openDialog, save as saveDialog, confirm } from "@tauri-apps/plugin-dialog";
import { S, h, icon, invoke, toast, fmtBytes, fmtUptime, modal, field, osIcon, statusDot } from "./ui.js";
import { activeSession, events } from "./panes.js";
import { t } from "./i18n.js";

let tab = "stats";
let pane = null;
let prev = null;
let timer = null;
let cwd = "";

export function initPanel() {
  const tabsEl = document.getElementById("panel-tabs");
  tabsEl.replaceChildren(...[["stats", t("stats")], ["sftp", t("sftp")]].map(([k, l]) =>
    h("button.seg" + (k === tab ? ".on" : ""), { role: "tab", "aria-selected": String(k === tab), onclick: () => { tab = k; initPanel(); refresh(); } }, l)));
}
events.addEventListener("focus", () => refresh());

function refresh() {
  const p = activeSession();
  if (p?.session !== pane?.session || p !== pane) { pane = p; prev = null; cwd = ""; }
  clearInterval(timer);
  const body = document.getElementById("panel-body");
  if (!S.settings.appearance.right_panel) return;
  if (!pane) return body.replaceChildren(h("p.empty", {}, t("no_session")));
  if (tab === "stats") { stats(); timer = setInterval(stats, 3000); }
  else sftp(cwd);
}

const meter = (label, pct, detail) => h("div.metric", {},
  h("div.metric-head", {}, h("span", {}, label), h("span.mono", {}, detail)),
  h("div.meter", { role: "meter", "aria-label": label, "aria-valuenow": Math.round(pct), "aria-valuemin": 0, "aria-valuemax": 100 },
    h("div", { style: `width:${Math.min(100, Math.max(0, pct)).toFixed(1)}%`, className: pct > 90 ? "hot" : pct > 70 ? "warm" : "" })));

function hostCard(st) {
  const host = pane.host;
  const [logo, brand] = osIcon(st?.os_id, st?.kernel, 26);
  const logoBox = h("div.os-logo", { style: `--brand:${brand}` }, logo);
  return h("div.host-card", {},
    logoBox,
    h("div.host-card-text", {},
      h("div.host-card-name", {}, statusDot(host.id), h("strong", {}, host.name)),
      st?.hostname && h("div.mono.mute", {}, st.hostname),
      st?.os_name && h("div.small.mute", {}, st.os_name)));
}

const kv = (k, v) => v && h("div.kv", {}, h("span.mute", {}, k), h("span.mono", {}, v));

async function stats() {
  const p = pane;
  if (!p?.session || document.hidden) return;
  let st;
  try { st = await invoke("ssh_stats", { id: p.session }); } catch { return; }
  if (p !== pane || tab !== "stats") return;
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
  const head = [hostCard(st), h("div.kvs", {}, kv(t("kernel"), st.kernel), kv(t("uptime"), st.uptime_secs ? fmtUptime(st.uptime_secs) : ""))];
  if (!st.mem_total_kb) return body.replaceChildren(...head, h("p.empty", {}, t("linux_only")));
  const memUsed = st.mem_total_kb - st.mem_avail_kb;
  body.replaceChildren(...head,
    h("div.metrics", {},
      meter(t("cpu"), cpu, prev && cpu ? `${cpu.toFixed(0)} %` : "…"),
      meter(t("memory"), (100 * memUsed) / st.mem_total_kb, `${fmtBytes(memUsed * 1024)} / ${fmtBytes(st.mem_total_kb * 1024)}`),
      meter(t("disk"), st.disk_total_kb ? (100 * st.disk_used_kb) / st.disk_total_kb : 0, `${fmtBytes(st.disk_used_kb * 1024)} / ${fmtBytes(st.disk_total_kb * 1024)}`)),
    h("div.tiles", {},
      h("div.tile", {}, h("span.mute", {}, t("network")), h("span.mono", {}, `↓ ${fmtBytes(rx)}/s`), h("span.mono", {}, `↑ ${fmtBytes(tx)}/s`)),
      h("div.tile", {}, h("span.mute", {}, t("load")), h("span.mono", {}, st.load.map((l) => l.toFixed(2)).join("  ")))),
  );
}

const join = (dir, name) => (dir.endsWith("/") ? dir : dir + "/") + name;

async function sftp(path) {
  const p = pane;
  const id = p?.session;
  if (!id) return;
  const body = document.getElementById("panel-body");
  let dir, entries;
  try { [dir, entries] = await invoke("sftp_list", { id, path }); }
  catch (e) { return body.replaceChildren(h("p.error", {}, "SFTP : " + e)); }
  if (p !== pane || tab !== "sftp") return;
  cwd = dir;
  const up = dir.replace(/\/[^/]+\/?$/, "") || "/";
  const act = (label, ic, fn) => h("button.icon-btn", { "aria-label": label, title: label, onclick: fn }, icon(ic, 15));
  body.replaceChildren(
    h("div.sftp-bar", {},
      act(t("parent_dir"), "up", () => sftp(up)),
      h("input.path", { value: dir, "aria-label": t("path"), onkeydown: (e) => e.key === "Enter" && sftp(e.target.value) }),
      act(t("refresh"), "refresh", () => sftp(dir)),
      act(t("upload"), "upload", async () => {
        const files = await openDialog({ multiple: true, title: t("upload_to", dir) });
        for (const f of [files].flat().filter(Boolean)) {
          const name = f.split(/[\\/]/).pop();
          try { await invoke("sftp_upload", { id, local: f, remote: join(dir, name) }); toast(t("sent", name), "success"); }
          catch (e) { toast(String(e), "error"); }
        }
        sftp(dir);
      }),
      act(t("mkdir"), "folderPlus", async () => {
        const name = h("input", { autofocus: true });
        const ok = await modal(t("mkdir"), (close) => h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field(t("folder_name"), name),
          h("footer", {}, h("button.btn", { type: "button", onclick: () => close(false) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("create")))));
        if (ok && name.value) invoke("sftp_mkdir", { id, path: join(dir, name.value) }).then(() => sftp(dir), (e) => toast(String(e), "error"));
      })),
    h("ul.files", { role: "list" }, ...entries.map((e) => h("li", {},
      h("button.row", { ondblclick: () => (e.dir ? sftp(join(dir, e.name)) : download(id, dir, e)), onkeydown: (ev) => ev.key === "Enter" && (e.dir ? sftp(join(dir, e.name)) : download(id, dir, e)), title: e.name },
        icon(e.dir ? "folder" : "file", 15), h("span.name", {}, e.name), h("span.mono.mute.size", {}, e.dir ? "" : fmtBytes(e.size))),
      !e.dir && act(t("download"), "download", () => download(id, dir, e)),
      act(t("delete"), "trash", async () => {
        if (!(await confirm(t("confirm_rm", e.name), { kind: "warning" }))) return;
        invoke("sftp_remove", { id, path: join(dir, e.name), dir: e.dir }).then(() => sftp(dir), (er) => toast(String(er), "error"));
      })))),
  );
}

async function download(id, dir, e) {
  const local = await saveDialog({ defaultPath: e.name, title: `${t("download")} ${e.name}` });
  if (!local) return;
  try { await invoke("sftp_download", { id, remote: join(dir, e.name), local }); toast(t("downloaded", e.name), "success"); }
  catch (er) { toast(String(er), "error"); }
}

export const refreshPanel = refresh;
