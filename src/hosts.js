// Sidebar: host tree (nested folders, drag & drop), search, tags, status dots, editor, import/export.
import { Menu } from "@tauri-apps/api/menu";
import { open as openDialog, save as saveDialog, confirm } from "@tauri-apps/plugin-dialog";
import { S, h, icon, invoke, modal, field, uid, toast, saveHosts, statusDot, HOST_ICONS } from "./ui.js";
import { t } from "./i18n.js";
import { wantsTmux } from "./panes.js";

let query = "";
let activeTag = null;
const collapsed = new Set(JSON.parse(safeGet("collapsed") || "[]"));
let onOpen = () => {};

function safeGet(k) { try { return localStorage.getItem(k); } catch { return null; } }
function safeSet(k, v) { try { localStorage.setItem(k, v); } catch {} }

export function initHosts(openHost) {
  onOpen = openHost;
  const search = document.getElementById("host-search");
  search.placeholder = t("search");
  search.setAttribute("aria-label", t("search_hosts"));
  search.addEventListener("input", () => { query = search.value.trim().toLowerCase(); render(); });
  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { const first = filtered()[0]; if (first) onOpen(first, true); }
    if (e.key === "Escape") { search.value = ""; query = ""; render(); search.blur(); }
  });
  const tree = document.getElementById("host-tree");
  dropTarget(tree, null); // dropping on empty space = move to root
  document.addEventListener("hosts-changed", render);
  render();
}

const matches = (x) =>
  (!activeTag || x.tags.includes(activeTag)) &&
  (!query || [x.name, x.host, x.user, x.description, ...x.tags].some((v) => v?.toLowerCase().includes(query)));
const filtered = () => S.hosts.hosts.filter(matches);

export function render() {
  const tags = [...new Set(S.hosts.hosts.flatMap((x) => x.tags))].sort();
  document.getElementById("tag-bar").replaceChildren(...tags.map((tg) =>
    h("button.chip" + (tg === activeTag ? ".on" : ""), { "aria-pressed": String(tg === activeTag), onclick: () => { activeTag = activeTag === tg ? null : tg; render(); } }, "#" + tg)));

  const tree = document.getElementById("host-tree");
  const flat = query || activeTag;
  const nodes = flat ? filtered().map((x) => hostRow(x, 0)) : folderChildren(null, 0);
  tree.replaceChildren(...(nodes.length ? nodes : [h("p.empty", {}, S.hosts.hosts.length || S.hosts.folders.length ? t("no_result") : t("empty_hosts"))]));
}

/** Only refresh the dots (called every probe round, keeps focus/scroll intact). */
export function renderStatus() {
  document.querySelectorAll("#host-tree [data-host]").forEach((row) => row.querySelector(".status")?.replaceWith(statusDot(row.dataset.host)));
}

function folderChildren(parent, depth) {
  const folders = S.hosts.folders.filter((f) => (f.parent ?? null) === parent).sort((a, b) => a.name.localeCompare(b.name));
  const hosts = S.hosts.hosts.filter((x) => (x.folder ?? null) === parent).sort((a, b) => a.name.localeCompare(b.name));
  return [...folders.map((f) => folderRow(f, depth)), ...hosts.map((x) => hostRow(x, depth))];
}

const countIn = (id) => S.hosts.hosts.filter((x) => x.folder === id).length +
  S.hosts.folders.filter((f) => f.parent === id).reduce((n, f) => n + countIn(f.id), 0);
const isAncestor = (a, f) => { while (f) { if (f === a) return true; f = S.hosts.folders.find((x) => x.id === f)?.parent; } return false; };

/** Accepts "host:<id>" / "folder:<id>" drags and moves them into `folderId` (null = root). */
function dropTarget(el, folderId) {
  el.addEventListener("dragover", (e) => {
    if (!e.dataTransfer.types.includes("text/plain")) return;
    e.preventDefault(); e.stopPropagation();
    e.dataTransfer.dropEffect = "move";
    document.querySelectorAll(".drop").forEach((d) => d !== el && d.classList.remove("drop"));
    el.classList.add("drop");
  });
  el.addEventListener("dragleave", (e) => { if (!el.contains(e.relatedTarget)) el.classList.remove("drop"); });
  el.addEventListener("drop", async (e) => {
    e.preventDefault(); e.stopPropagation();
    el.classList.remove("drop");
    const [kind, id] = e.dataTransfer.getData("text/plain").split(":");
    if (kind === "host") {
      const x = S.hosts.hosts.find((y) => y.id === id);
      if (!x || (x.folder ?? null) === folderId) return;
      x.folder = folderId;
    } else if (kind === "folder") {
      const f = S.hosts.folders.find((y) => y.id === id);
      if (!f || id === folderId || isAncestor(id, folderId) || (f.parent ?? null) === folderId) return;
      f.parent = folderId;
      collapsed.delete(folderId);
    } else return;
    await saveHosts();
  });
}

function draggable(el, payload) {
  el.draggable = true;
  el.addEventListener("dragstart", (e) => {
    e.dataTransfer.setData("text/plain", payload);
    e.dataTransfer.effectAllowed = "move";
    el.classList.add("dragging");
    document.body.classList.add("is-dragging");
  });
  el.addEventListener("dragend", () => {
    el.classList.remove("dragging");
    document.body.classList.remove("is-dragging");
    document.querySelectorAll(".drop").forEach((d) => d.classList.remove("drop"));
  });
}

function folderRow(f, depth) {
  const open = !collapsed.has(f.id);
  const toggle = () => { open ? collapsed.add(f.id) : collapsed.delete(f.id); safeSet("collapsed", JSON.stringify([...collapsed])); render(); };
  const row = h("button.row.folder" + (open ? ".open" : ""), {
    "aria-expanded": String(open), onclick: toggle, style: `--depth:${depth}`,
    onkeydown: (e) => { if ((e.key === "ArrowRight" && !open) || (e.key === "ArrowLeft" && open)) { e.preventDefault(); toggle(); } },
    oncontextmenu: (e) => { e.preventDefault(); folderMenu(f); },
  }, h("span.chev", {}, icon("chevron", 14)), icon(open ? "folderOpen" : "folder", 15), h("span.name", {}, f.name), h("span.count", {}, String(countIn(f.id))));
  draggable(row, "folder:" + f.id);
  const group = h("div.group", {}, row, open && h("div.children", {}, ...folderChildren(f.id, depth + 1)));
  dropTarget(group, f.id);
  return group;
}

function hostRow(x, depth) {
  const sub = `${x.user ? x.user + "@" : ""}${x.host}${x.port && x.port !== 22 ? ":" + x.port : ""}`;
  const row = h("button.row.host", {
    dataset: { host: x.id }, style: `--depth:${depth}`,
    title: `${sub}${x.description ? "\n" + x.description : ""}`,
    ondblclick: () => onOpen(x, true),
    onkeydown: (e) => e.key === "Enter" && (e.preventDefault(), onOpen(x, true)),
    oncontextmenu: (e) => { e.preventDefault(); hostMenu(x); },
  }, statusDot(x.id), icon(x.icon || "server", 15),
    h("span.name", {}, x.name || x.host),
    x.favorite && h("span.fav", { "aria-label": t("favorite") }, icon("star", 12)),
    ...x.tags.slice(0, 2).map((tg) => h("span.tag", {}, tg)));
  draggable(row, "host:" + x.id);
  // Dropping onto a host = move into the host's folder.
  dropTarget(row, x.folder ?? null);
  return row;
}

async function hostMenu(x) {
  const m = await Menu.new({ items: [
    { id: "o", text: t("connect_tab"), action: () => onOpen(x, true) },
    // The other way than usual, for this connection only.
    wantsTmux(x) ? { id: "nt", text: t("connect_no_tmux"), action: () => onOpen(x, { tmux: false }) }
      : { id: "wt", text: t("connect_tmux"), action: () => onOpen(x, { tmux: true }) },
    { id: "e", text: t("edit"), action: () => editHost(x) },
    { id: "d", text: t("duplicate"), action: async () => { S.hosts.hosts.push({ ...structuredClone(x), id: uid(), name: `${x.name} ${t("copy_suffix")}` }); await saveHosts(); } },
    { id: "f", text: x.favorite ? t("rm_fav") : t("add_fav"), action: async () => { x.favorite = !x.favorite; await saveHosts(); } },
    { item: "Separator" },
    { id: "r", text: t("delete"), action: () => removeHost(x) },
  ] });
  m.popup();
}

async function folderMenu(f) {
  const m = await Menu.new({ items: [
    { id: "n", text: t("new_host_here"), action: () => editHost(null, f.id) },
    { id: "s", text: t("new_subfolder"), action: () => editFolder(null, f.id) },
    { id: "e", text: t("rename"), action: () => editFolder(f) },
    { item: "Separator" },
    { id: "r", text: t("delete_folder"), action: async () => {
      if (!(await confirm(t("confirm_folder", f.name), { kind: "warning" }))) return;
      S.hosts.hosts.forEach((x) => x.folder === f.id && (x.folder = f.parent ?? null));
      S.hosts.folders.forEach((c) => c.parent === f.id && (c.parent = f.parent ?? null));
      S.hosts.folders = S.hosts.folders.filter((c) => c !== f);
      await saveHosts();
    } },
  ] });
  m.popup();
}

async function removeHost(x) {
  if (!(await confirm(t("confirm_host", x.name), { kind: "warning" }))) return;
  S.hosts.hosts = S.hosts.hosts.filter((y) => y !== x);
  await invoke("set_secret", { id: x.id, value: "" }).catch(() => {});
  await invoke("set_secret", { id: x.id + ":passphrase", value: "" }).catch(() => {});
  await saveHosts();
}

/** Folder path for selects, e.g. "Prod / Web". */
const folderPath = (f) => { const p = []; while (f) { p.unshift(f.name); f = S.hosts.folders.find((x) => x.id === f.parent); } return p.join(" / "); };
const folderOptions = (selected, exclude = null) => [
  h("option", { value: "" }, t("root")),
  ...S.hosts.folders.filter((f) => !exclude || (f.id !== exclude && !isAncestor(exclude, f.id)))
    .map((f) => [folderPath(f), f]).sort((a, b) => a[0].localeCompare(b[0]))
    .map(([p, f]) => h("option", { value: f.id, selected: f.id === selected }, p)),
];

export async function editFolder(f, parent = null) {
  const name = h("input", { value: f?.name || "", required: true, autofocus: true });
  const par = h("select", {}, ...folderOptions(f ? f.parent : parent, f?.id));
  const ok = await modal(f ? t("rename_folder") : t("new_folder"), (close) =>
    h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field(t("folder_name"), name), field(t("parent_folder"), par),
      h("footer", {}, h("button.btn", { type: "button", onclick: () => close(false) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("save")))));
  if (!ok || !name.value.trim()) return;
  if (f) Object.assign(f, { name: name.value.trim(), parent: par.value || null });
  else S.hosts.folders.push({ id: uid(), name: name.value.trim(), parent: par.value || null });
  if (par.value) collapsed.delete(par.value);
  await saveHosts();
}

export async function editHost(x = null, folder = null) {
  const isNew = !x;
  x = x ? structuredClone(x) : { id: uid(), name: "", host: "", port: 22, user: "", key_path: "", description: "", icon: "server", tags: [], folder, favorite: false, tmux: "" };
  const hasPw = !isNew && (await invoke("has_secret", { id: x.id }).catch(() => false));
  const vaultKeys = await invoke("sync_keys").catch(() => []);
  const inp = (k, attrs = {}) => h("input", { value: x[k] ?? "", ...attrs });
  const f = {
    name: inp("name", { placeholder: "web-1", autofocus: true }),
    host: inp("host", { placeholder: "192.168.1.10 / example.com", required: true }),
    port: inp("port", { type: "number", min: 1, max: 65535 }),
    user: inp("user", { placeholder: "root" }),
    key_path: inp("key_path", { placeholder: "~/.ssh/id_ed25519" }),
    password: h("input", { type: "password", placeholder: hasPw ? t("f_pass_saved") : t("f_optional") }),
    passphrase: h("input", { type: "password", placeholder: t("f_passphrase_ph") }),
    description: h("textarea", { rows: 2 }, x.description),
    tags: h("input", { value: x.tags.join(", "), placeholder: "prod, web" }),
    favorite: h("input", { type: "checkbox", checked: x.favorite, className: "switch" }),
    folder: h("select", {}, ...folderOptions(x.folder)),
    tmux: h("select", {}, ...[["", t(S.settings.terminal.tmux ? "tmux_host_default_on" : "tmux_host_default_off")], ["on", t("tmux_host_on")], ["off", t("tmux_host_off")]]
      .map(([v, l]) => h("option", { value: v, selected: v === (x.tmux || "") }, l))),
    key_id: h("select", {}, h("option", { value: "" }, t("f_vault_key_none")),
      ...vaultKeys.map((k) => h("option", { value: k.id, selected: k.id === x.key_id }, k.name || k.id))),
  };
  let chosenIcon = x.icon || "server";
  const icons = h("div.icon-grid", { role: "radiogroup", "aria-label": t("f_icon") }, ...Object.keys(HOST_ICONS).map((k) =>
    h("button.icon-choice" + (k === chosenIcon ? ".on" : ""), { type: "button", role: "radio", "aria-checked": String(k === chosenIcon), title: k, "aria-label": k,
      onclick: (e) => { chosenIcon = k; icons.querySelectorAll(".icon-choice").forEach((b) => { b.classList.toggle("on", b === e.currentTarget); b.setAttribute("aria-checked", String(b === e.currentTarget)); }); } }, icon(k, 18))));

  const browse = h("button.btn.small", { type: "button", onclick: async () => {
    const p = await openDialog({ title: t("pick_key"), defaultPath: "~/.ssh" });
    if (p) f.key_path.value = p;
  } }, t("browse"));
  const importBtn = h("button.btn.small", { type: "button", onclick: async () => {
    const p = await openDialog({ title: t("import_key") });
    if (p) try { f.key_path.value = await invoke("import_key", { path: p }); toast(t("key_imported"), "success"); } catch (e) { toast(String(e), "error"); }
  } }, t("import"));

  const ok = await modal(isNew ? t("host_new") : t("host_edit"), (close) => h("form.grid", { onsubmit: (e) => { e.preventDefault(); close(true); } },
    field(t("f_name"), f.name), field(t("f_host"), f.host),
    field(t("f_user"), f.user), field(t("f_port"), f.port),
    h("div.span2", {}, field(t("f_key"), h("div.inline", {}, f.key_path, browse, importBtn), t("f_key_hint"))),
    vaultKeys.length ? h("div.span2", {}, field(t("f_vault_key"), f.key_id, t("f_vault_key_hint"))) : null,
    field(t("f_password"), f.password, t("f_password_hint")), field(t("f_passphrase"), f.passphrase),
    h("div.span2", {}, field(t("f_desc"), f.description)),
    field(t("f_tags"), f.tags, t("f_tags_hint")), field(t("f_folder"), f.folder),
    h("div.span2", {}, field(t("tmux_setting"), f.tmux, t("tmux_host_hint"))),
    h("div.span2", {}, field(t("f_icon"), icons)),
    h("label.toggle.span2", {}, h("span", {}, t("f_fav")), f.favorite),
    h("footer.span2", {}, h("button.btn", { type: "button", onclick: () => close(false) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("save")))), { wide: true });
  if (!ok) return;

  const port = Number(f.port.value) || 22;
  if (!f.host.value.trim() || port < 1 || port > 65535) return toast(t("bad_host"), "error");
  Object.assign(x, {
    name: f.name.value.trim() || f.host.value.trim(), host: f.host.value.trim(), port, user: f.user.value.trim(),
    key_path: f.key_path.value.trim(), key_id: f.key_id.value, description: f.description.value, icon: chosenIcon,
    tags: f.tags.value.split(",").map((s) => s.trim()).filter(Boolean), folder: f.folder.value || null, favorite: f.favorite.checked, tmux: f.tmux.value,
  });
  try {
    if (f.password.value) await invoke("set_secret", { id: x.id, value: f.password.value });
    if (f.passphrase.value) await invoke("set_secret", { id: x.id + ":passphrase", value: f.passphrase.value });
  } catch (e) { toast(`${t("keychain")} : ${e}`, "error"); }
  const i = S.hosts.hosts.findIndex((y) => y.id === x.id);
  i < 0 ? S.hosts.hosts.push(x) : (S.hosts.hosts[i] = x);
  await saveHosts();
}

/** Export/import as JSON. Secrets stay in the OS keychain and are not exported. */
export async function exportHosts() {
  const path = await saveDialog({ title: t("export_hosts"), defaultPath: "termix-hosts.json", filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path) return;
  try { await invoke("export_hosts", { path }); toast(t("exported"), "success"); } catch (e) { toast(String(e), "error"); }
}

export async function importHosts() {
  const path = await openDialog({ title: t("import_hosts"), filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path) return;
  try {
    const doc = await invoke("read_hosts_file", { path });
    const known = new Set(S.hosts.hosts.map((x) => x.id));
    const kf = new Set(S.hosts.folders.map((x) => x.id));
    S.hosts.folders.push(...doc.folders.filter((x) => !kf.has(x.id)));
    const added = doc.hosts.filter((x) => !known.has(x.id));
    S.hosts.hosts.push(...added);
    await saveHosts();
    toast(t("imported", added.length), "success");
  } catch (e) { toast(String(e), "error"); }
}
