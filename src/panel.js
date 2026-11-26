// Right panel: server stats + SFTP browser for the active session.
import { open as openDialog, save as saveDialog, confirm } from "@tauri-apps/plugin-dialog";
import { S, h, icon, invoke, toast, fmtBytes, fmtUptime, modal, field, osIcon, osKey, statusDot, saveHosts, contextMenu } from "./ui.js";
import { activeSession, events, newTab } from "./panes.js";
import { t, lang } from "./i18n.js";

let tab = "stats";
let pane = null;
let prev = null;
let timer = null;
let cwd = "";

export function initPanel() {
  const tabsEl = document.getElementById("panel-tabs");
  // Icons, not words: four labels don't fit the panel header in every language.
  tabsEl.replaceChildren(...[["stats", "stats"], ["sftp", "folderTree"], ["docker", "docker"], ["tmux", "terminal"]].map(([k, ic]) =>
    h("button.seg" + (k === tab ? ".on" : ""), { role: "tab", "aria-selected": String(k === tab), "aria-label": t(k), title: t(k), onclick: () => { tab = k; initPanel(); refresh(); } }, icon(ic, 15))));
}
events.addEventListener("focus", () => refresh());

function refresh() {
  const p = activeSession();
  if (p?.session !== pane?.session || p !== pane) { pane = p; prev = null; cwd = ""; }
  clearInterval(timer);
  const body = document.getElementById("panel-body");
  if (!S.settings.appearance.right_panel) return;
  if (!pane) return body.replaceChildren(h("p.empty", {}, t("no_session")));
  if (tab === "stats") { stats(); timer = setInterval(stats, (S.settings.performance.stats_secs || 3) * 1000); }
  else if (tab === "docker") { docker(); timer = setInterval(docker, 5000); }
  else if (tab === "tmux") { tmux(); timer = setInterval(tmux, 5000); }
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
  // Remembered on the host for its "os:auto" icon (sidebar, pickers).
  const os = osKey(st.os_id, st.kernel), saved = S.hosts.hosts.find((x) => x.id === p.host.id);
  if (os && saved && saved.os !== os) { saved.os = os; saveHosts().catch(() => {}); }
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
const parentOf = (dir) => dir.replace(/\/[^/]+\/?$/, "") || "/";
/** Single-quoted for the remote shell. */
const shq = (v) => "'" + v.replace(/'/g, `'\\''`) + "'";
const modeStr = (m) => [..."rwxrwxrwx"].map((c, i) => (m & (0o400 >> i) ? c : "-")).join("");

// View options, kept between launches (this device only).
const sv = { hidden: false, sort: "name", desc: false };
try { Object.assign(sv, JSON.parse(localStorage.getItem("sftp") || "{}")); } catch {}
const saveView = () => { try { localStorage.setItem("sftp", JSON.stringify(sv)); } catch {} };
let filter = "";

function fmtDate(secs) {
  if (!secs) return "";
  const d = new Date(secs * 1000), thisYear = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleString(lang, thisYear ? { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" } : { day: "numeric", month: "short", year: "numeric" });
}

/** One-field dialog; resolves with the trimmed value, or null. */
async function ask(title, label, value = "", hint) {
  const input = h("input", { value, autofocus: true });
  const ok = await modal(title, (close) => h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field(label, input, hint),
    h("footer", {}, h("button.btn", { type: "button", onclick: () => close(false) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("save")))));
  return ok && input.value.trim() ? input.value.trim() : null;
}
const run = (p, after) => p.then(after, (e) => toast(String(e), "error"));

async function sftp(path) {
  const p = pane;
  const id = p?.session;
  if (!id) return;
  const body = document.getElementById("panel-body");
  let dir, entries;
  try { [dir, entries] = await invoke("sftp_list", { id, path }); }
  catch (e) { return body.replaceChildren(h("p.error", {}, "SFTP : " + e), h("div.actions", {}, h("button.btn.small", { onclick: () => sftp(cwd) }, t("retry")))); }
  if (p !== pane || tab !== "sftp") return;
  if (dir !== cwd) filter = "";
  cwd = dir;
  const act = (label, ic, fn, pressed) => h("button.icon-btn" + (pressed ? ".on" : ""), { "aria-label": label, title: label, "aria-pressed": pressed == null ? null : String(pressed), onclick: fn }, icon(ic, 15));
  const reload = () => sftp(dir);
  const list = h("ul.files", { role: "list" });
  const cols = h("div.cols");
  const foot = h("p.files-foot.mute.small");

  const open = (e) => (e.dir ? sftp(join(dir, e.name)) : download(id, dir, e));
  const rename = async (e) => {
    const to = await ask(t("sftp_rename"), t("sftp_new_name"), e.name);
    if (to && to !== e.name) run(invoke("sftp_rename", { id, from: join(dir, e.name), to: to.startsWith("/") ? to : join(dir, to) }), reload);
  };
  const chmod = async (e) => {
    const v = await ask(t("sftp_perms"), e.name, (e.perm & 0o7777).toString(8).padStart(3, "0"), `${modeStr(e.perm)} — ${t("sftp_perms_hint")}`);
    if (v == null) return;
    if (!/^[0-7]{3,4}$/.test(v)) return toast(t("sftp_perms_bad"), "error");
    run(invoke("sftp_chmod", { id, path: join(dir, e.name), mode: parseInt(v, 8) }), reload);
  };
  const remove = async (e) => {
    if (!(await confirm(t(e.dir ? "confirm_rm_dir" : "confirm_rm", e.name), { kind: "warning" }))) return;
    run(invoke("sftp_remove", { id, path: join(dir, e.name), dir: e.dir }), reload);
  };
  // Types `cd` into this pane's shell: the terminal follows the browser.
  const cdHere = (path) => invoke("ssh_write", { id, data: ` cd -- ${shq(path)}\r` }).catch((er) => toast(String(er), "error"));
  const menu = (ev, e) => contextMenu(ev, [
    e.dir ? { label: t("sftp_open"), icon: "folderOpen", action: () => open(e) } : { label: t("download"), icon: "download", action: () => download(id, dir, e) },
    { label: t("sftp_rename"), icon: "edit", action: () => rename(e) },
    { label: t("sftp_perms"), icon: "lock", action: () => chmod(e) },
    { label: t("sftp_copy_path"), icon: "copy", action: () => navigator.clipboard.writeText(join(dir, e.name)) },
    { label: t("sftp_cd"), icon: "cdHere", action: () => cdHere(e.dir ? join(dir, e.name) : dir) },
    "-",
    { label: t("delete"), icon: "trash", danger: true, action: () => remove(e) },
  ]);

  const draw = () => {
    const q = filter.toLowerCase();
    const shown = entries.filter((e) => (sv.hidden || !e.name.startsWith(".")) && (!q || e.name.toLowerCase().includes(q)));
    const by = { name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }), size: (a, b) => a.size - b.size, date: (a, b) => a.modified - b.modified }[sv.sort] || (() => 0);
    shown.sort((a, b) => b.dir - a.dir || (sv.desc ? -1 : 1) * by(a, b));
    const col = (k, label) => h("button.col." + k + (sv.sort === k ? ".on" : ""), { type: "button", "aria-pressed": String(sv.sort === k),
      onclick: () => { sv.desc = sv.sort === k && !sv.desc; sv.sort = k; saveView(); draw(); } }, label, sv.sort === k ? (sv.desc ? " ↓" : " ↑") : "");
    cols.replaceChildren(col("name", t("sftp_name")), col("size", t("sftp_size")), col("date", t("sftp_date")));
    list.replaceChildren(...shown.map((e) => h("li", { oncontextmenu: (ev) => { ev.preventDefault(); menu(ev, e); } },
      h("button.row", { title: [e.name, e.perm ? modeStr(e.perm) : "", fmtDate(e.modified)].filter(Boolean).join("\n"), ondblclick: () => open(e),
        onkeydown: (ev) => {
          if (ev.key === "Enter") open(e);
          else if (ev.key === "F2") rename(e);
          else if (ev.key === "Delete" || (ev.key === "Backspace" && (ev.metaKey || ev.ctrlKey))) remove(e);
          else return;
          ev.preventDefault();
        } },
        icon(e.dir ? "folder" : e.link ? "link" : "file", 15), h("span.name", {}, e.name),
        h("span.mono.mute.size", {}, e.dir ? "" : fmtBytes(e.size)), h("span.mono.mute.date", {}, fmtDate(e.modified))),
      act(t("more"), "more", (ev) => menu(ev, e)))));
    const hidden = entries.filter((e) => e.name.startsWith(".")).length;
    foot.textContent = t("sftp_count", shown.length) + (!sv.hidden && hidden ? " · " + t("sftp_hidden_n", hidden) : "");
    if (!shown.length) list.append(h("li.empty", {}, entries.length ? t("no_result") : t("sftp_empty")));
  };

  const hiddenBtn = act(t("sftp_hidden"), sv.hidden ? "eye" : "eyeOff", () => {
    sv.hidden = !sv.hidden;
    saveView();
    hiddenBtn.replaceChildren(icon(sv.hidden ? "eye" : "eyeOff", 15));
    hiddenBtn.classList.toggle("on", sv.hidden);
    hiddenBtn.setAttribute("aria-pressed", String(sv.hidden));
    draw();
  }, sv.hidden);
  body.replaceChildren(
    h("div.sftp-bar", {},
      act(t("parent_dir"), "up", () => sftp(parentOf(dir))),
      act(t("sftp_home"), "home", () => sftp("")),
      h("input.path", { value: dir, "aria-label": t("path"), onkeydown: (e) => e.key === "Enter" && sftp(e.target.value) }),
      act(t("refresh"), "refresh", reload)),
    h("div.sftp-bar", {},
      h("input.path.filter", { type: "search", value: filter, placeholder: t("sftp_filter"), "aria-label": t("sftp_filter"), oninput: (e) => { filter = e.target.value; draw(); } }),
      hiddenBtn,
      act(t("sftp_new_file"), "filePlus", async () => {
        const name = await ask(t("sftp_new_file"), t("sftp_file_name"));
        if (name) run(invoke("sftp_touch", { id, path: join(dir, name) }), reload);
      }),
      act(t("mkdir"), "folderPlus", async () => {
        const name = await ask(t("mkdir"), t("folder_name"));
        if (name) run(invoke("sftp_mkdir", { id, path: join(dir, name) }), reload);
      }),
      act(t("upload"), "upload", async () => {
        const files = await openDialog({ multiple: true, title: t("upload_to", dir) });
        for (const f of [files].flat().filter(Boolean)) {
          const name = f.split(/[\\/]/).pop();
          try { await invoke("sftp_upload", { id, local: f, remote: join(dir, name) }); toast(t("sent", name), "success"); }
          catch (e) { toast(String(e), "error"); }
        }
        reload();
      })),
    cols, list, foot,
  );
  draw();
}

async function download(id, dir, e) {
  const local = await saveDialog({ defaultPath: e.name, title: `${t("download")} ${e.name}` });
  if (!local) return;
  try { await invoke("sftp_download", { id, remote: join(dir, e.name), local }); toast(t("downloaded", e.name), "success"); }
  catch (er) { toast(String(er), "error"); }
}

// ---------- docker ----------

const DOT = { running: "up", restarting: "ping", paused: "ping", dead: "down" };

async function docker() {
  const p = pane;
  if (!p?.session || document.hidden) return;
  const body = document.getElementById("panel-body");
  let d;
  try { d = await invoke("docker_ps", { id: p.session }); } catch (e) { return body.replaceChildren(h("p.error", {}, String(e))); }
  if (p !== pane || tab !== "docker") return;
  if (!d.installed) return body.replaceChildren(h("p.empty", {}, t("no_docker")));
  if (d.error) return body.replaceChildren(h("p.error.small", {}, d.error));
  const up = d.containers.filter((c) => c.state === "running").length;
  body.replaceChildren(
    h("div.list-head", {}, h("span", {}, t("containers")), h("span.mono.mute", {}, `${up} / ${d.containers.length}`)),
    d.containers.length
      ? h("ul.items", { role: "list" }, ...d.containers.map((c) => h("li.item", { title: `${c.name} · ${c.status}` },
        h("span.status" + (DOT[c.state] ? "." + DOT[c.state] : ""), { role: "img", "aria-label": c.state }),
        h("div.item-text", {}, h("span.item-name", {}, c.name), h("span.item-meta", {}, `${c.image.split("/").pop()} · ${c.status}`)))))
      : h("p.empty", {}, t("no_containers")));
}

// ---------- tmux ----------

async function tmux() {
  const p = pane;
  if (!p?.session || document.hidden) return;
  const body = document.getElementById("panel-body");
  let list;
  try { list = await invoke("tmux_list", { id: p.session }); } catch (e) { return body.replaceChildren(h("p.error", {}, String(e))); }
  if (p !== pane || tab !== "tmux") return;
  if (!list) return body.replaceChildren(h("p.empty", {}, t("no_tmux")));
  const now = Date.now() / 1000;
  const kill = (s) => h("button.icon-btn", { "aria-label": `${t("tmux_kill")} ${s.name}`, title: t("tmux_kill"), onclick: async () => {
    if (!(await confirm(t("tmux_confirm_kill", s.name), { kind: "warning" }))) return;
    invoke("tmux_kill", { id: p.session, name: s.name }).then(tmux, (e) => toast(String(e), "error"));
  } }, icon("trash", 15));
  body.replaceChildren(
    h("div.list-head", {}, h("span", {}, t("tmux_sessions")), h("span.mono.mute", {}, String(list.length))),
    list.length
      ? h("ul.items", { role: "list" }, ...list.map((s) => {
        const here = s.name === p.tmux;
        const meta = [s.command, t("tmux_windows", s.windows), s.activity ? t("ago", fmtUptime(Math.max(60, now - s.activity))) : ""].filter(Boolean).join(" · ");
        return h("li.item", {},
          h("button.item-main", { disabled: here, title: here ? t("tmux_here") : t("tmux_resume"), onclick: () => newTab(p.host, s.name) },
            icon("terminal", 15),
            h("div.item-text", {}, h("span.item-name.mono", {}, s.name), h("span.item-meta", {}, here ? t("tmux_here") : s.attached ? `${t("tmux_attached")} · ${meta}` : meta))),
          !here && kill(s));
      }))
      : h("p.empty", {}, t("no_tmux_sessions")),
    h("p.mute.small", {}, S.settings.terminal.tmux ? t("tmux_hint") : t("tmux_off_hint")));
}

export const refreshPanel = refresh;
