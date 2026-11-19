// Sidebar: host tree (folders), search, tags, editor, import/export.
import { Menu } from "@tauri-apps/api/menu";
import { open as openDialog, save as saveDialog, confirm } from "@tauri-apps/plugin-dialog";
import { S, h, icon, invoke, modal, field, uid, toast, saveHosts, HOST_ICONS } from "./ui.js";

let query = "";
let activeTag = null;
const collapsed = new Set(JSON.parse(safeGet("collapsed") || "[]"));
let onOpen = () => {};

function safeGet(k) { try { return localStorage.getItem(k); } catch { return null; } }
function safeSet(k, v) { try { localStorage.setItem(k, v); } catch {} }

export function initHosts(openHost) {
  onOpen = openHost;
  const search = document.getElementById("host-search");
  search.addEventListener("input", () => { query = search.value.trim().toLowerCase(); render(); });
  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { const first = filtered()[0]; if (first) onOpen(first); }
    if (e.key === "Escape") { search.value = ""; query = ""; render(); search.blur(); }
  });
  document.getElementById("add-host").append(icon("plus"));
  document.getElementById("add-host").onclick = () => editHost();
  document.getElementById("add-folder").append(icon("folderPlus"));
  document.getElementById("add-folder").onclick = () => editFolder();
  document.addEventListener("hosts-changed", render);
  render();
}

const matches = (x) =>
  (!activeTag || x.tags.includes(activeTag)) &&
  (!query || [x.name, x.host, x.user, x.description, ...x.tags].some((v) => v?.toLowerCase().includes(query)));
const filtered = () => S.hosts.hosts.filter(matches);

export function render() {
  const tags = [...new Set(S.hosts.hosts.flatMap((x) => x.tags))].sort();
  document.getElementById("tag-bar").replaceChildren(...tags.map((t) =>
    h("button.chip" + (t === activeTag ? ".on" : ""), { "aria-pressed": String(t === activeTag), onclick: () => { activeTag = activeTag === t ? null : t; render(); } }, "#" + t)));

  const tree = document.getElementById("host-tree");
  const flat = query || activeTag;
  const nodes = flat ? filtered().map(hostRow) : folderChildren(null);
  tree.replaceChildren(...(nodes.length ? nodes : [h("p.empty", {}, S.hosts.hosts.length ? "Aucun résultat" : "Aucun hôte. Ajoutez-en un avec +")]));
}

function folderChildren(parent) {
  const folders = S.hosts.folders.filter((f) => (f.parent ?? null) === parent).sort((a, b) => a.name.localeCompare(b.name));
  const hosts = S.hosts.hosts.filter((x) => (x.folder ?? null) === parent).sort((a, b) => a.name.localeCompare(b.name));
  return [...folders.map(folderRow), ...hosts.map(hostRow)];
}

function dropTarget(el, folderId) {
  el.addEventListener("dragover", (e) => { e.preventDefault(); el.classList.add("drop"); });
  el.addEventListener("dragleave", () => el.classList.remove("drop"));
  el.addEventListener("drop", async (e) => {
    e.preventDefault(); el.classList.remove("drop");
    const [kind, id] = e.dataTransfer.getData("text/plain").split(":");
    if (kind === "host") S.hosts.hosts.find((x) => x.id === id).folder = folderId;
    else if (kind === "folder" && id !== folderId && !isAncestor(id, folderId)) S.hosts.folders.find((f) => f.id === id).parent = folderId;
    await saveHosts();
  });
}
const isAncestor = (a, f) => { while (f) { if (f === a) return true; f = S.hosts.folders.find((x) => x.id === f)?.parent; } return false; };

function folderRow(f) {
  const open = !collapsed.has(f.id);
  const toggle = () => { open ? collapsed.add(f.id) : collapsed.delete(f.id); safeSet("collapsed", JSON.stringify([...collapsed])); render(); };
  const row = h("button.row.folder", {
    draggable: true, "aria-expanded": String(open), onclick: toggle,
    ondragstart: (e) => e.dataTransfer.setData("text/plain", "folder:" + f.id),
    oncontextmenu: (e) => { e.preventDefault(); folderMenu(f); },
  }, icon(open ? "folderOpen" : "folder"), h("span.name", {}, f.name));
  dropTarget(row, f.id);
  return h("div.group", {}, row, open && h("div.children", {}, ...folderChildren(f.id)));
}

function hostRow(x) {
  return h("button.row.host", {
    draggable: true, title: `${x.user ? x.user + "@" : ""}${x.host}:${x.port || 22}${x.description ? "\n" + x.description : ""}`,
    ondblclick: () => onOpen(x, true), onclick: () => onOpen(x),
    onkeydown: (e) => e.key === "Enter" && (e.preventDefault(), onOpen(x, true)),
    ondragstart: (e) => e.dataTransfer.setData("text/plain", "host:" + x.id),
    oncontextmenu: (e) => { e.preventDefault(); hostMenu(x); },
  }, icon(x.icon || "server"), h("span.name", {}, x.name || x.host), x.favorite && h("span.fav", { "aria-label": "favori" }, icon("star", 12)),
    ...x.tags.slice(0, 2).map((t) => h("span.tag", {}, t)));
}

async function hostMenu(x) {
  const m = await Menu.new({ items: [
    { id: "o", text: "Se connecter (nouvel onglet)", action: () => onOpen(x, true) },
    { id: "e", text: "Modifier…", action: () => editHost(x) },
    { id: "d", text: "Dupliquer", action: async () => { S.hosts.hosts.push({ ...structuredClone(x), id: uid(), name: x.name + " (copie)" }); await saveHosts(); } },
    { id: "f", text: x.favorite ? "Retirer des favoris" : "Ajouter aux favoris", action: async () => { x.favorite = !x.favorite; await saveHosts(); } },
    { item: "Separator" },
    { id: "r", text: "Supprimer…", action: () => removeHost(x) },
  ] });
  m.popup();
}

async function folderMenu(f) {
  const m = await Menu.new({ items: [
    { id: "n", text: "Nouvel hôte ici…", action: () => editHost(null, f.id) },
    { id: "s", text: "Nouveau sous-dossier…", action: () => editFolder(null, f.id) },
    { id: "e", text: "Renommer…", action: () => editFolder(f) },
    { item: "Separator" },
    { id: "r", text: "Supprimer le dossier…", action: async () => {
      if (!(await confirm(`Supprimer « ${f.name} » ? Son contenu remonte d'un niveau.`, { kind: "warning" }))) return;
      S.hosts.hosts.forEach((x) => x.folder === f.id && (x.folder = f.parent ?? null));
      S.hosts.folders.forEach((c) => c.parent === f.id && (c.parent = f.parent ?? null));
      S.hosts.folders = S.hosts.folders.filter((c) => c !== f);
      await saveHosts();
    } },
  ] });
  m.popup();
}

async function removeHost(x) {
  if (!(await confirm(`Supprimer l'hôte « ${x.name} » ?`, { kind: "warning" }))) return;
  S.hosts.hosts = S.hosts.hosts.filter((y) => y !== x);
  await invoke("set_secret", { id: x.id, value: "" });
  await invoke("set_secret", { id: x.id + ":passphrase", value: "" });
  await saveHosts();
}

export async function editFolder(f, parent = null) {
  const name = h("input", { value: f?.name || "", required: true, autofocus: true });
  const ok = await modal(f ? "Renommer le dossier" : "Nouveau dossier", (close) =>
    h("form", { onsubmit: (e) => { e.preventDefault(); close(true); } }, field("Nom", name),
      h("footer", {}, h("button.btn.primary", { type: "submit" }, "Enregistrer"))));
  if (!ok || !name.value.trim()) return;
  if (f) f.name = name.value.trim();
  else S.hosts.folders.push({ id: uid(), name: name.value.trim(), parent });
  await saveHosts();
}

export async function editHost(x = null, folder = null) {
  const isNew = !x;
  x = x ? structuredClone(x) : { id: uid(), name: "", host: "", port: 22, user: "", key_path: "", description: "", icon: "server", tags: [], folder, favorite: false };
  const hasPw = !isNew && (await invoke("has_secret", { id: x.id }));
  const inp = (k, attrs = {}) => h("input", { value: x[k] ?? "", ...attrs });
  const f = {
    name: inp("name", { placeholder: "Mon serveur", autofocus: true }),
    host: inp("host", { placeholder: "192.168.1.10 ou exemple.com", required: true, spellcheck: false }),
    port: inp("port", { type: "number", min: 1, max: 65535 }),
    user: inp("user", { placeholder: "root", spellcheck: false }),
    key_path: inp("key_path", { placeholder: "~/.ssh/id_ed25519 (optionnel)", spellcheck: false }),
    password: h("input", { type: "password", placeholder: hasPw ? "•••••• (enregistré, laisser vide pour garder)" : "Optionnel", autocomplete: "off" }),
    passphrase: h("input", { type: "password", placeholder: "Phrase de passe de la clé (optionnel)", autocomplete: "off" }),
    description: h("textarea", { rows: 2 }, x.description),
    tags: h("input", { value: x.tags.join(", "), placeholder: "prod, web, perso" }),
    favorite: h("input", { type: "checkbox", checked: x.favorite }),
    folder: h("select", {}, h("option", { value: "" }, "— Racine —"),
      ...S.hosts.folders.map((fo) => h("option", { value: fo.id, selected: fo.id === x.folder }, fo.name))),
  };
  let chosenIcon = x.icon || "server";
  const icons = h("div.icon-grid", { role: "radiogroup", "aria-label": "Icône" }, ...Object.keys(HOST_ICONS).map((k) =>
    h("button.icon-choice" + (k === chosenIcon ? ".on" : ""), { type: "button", role: "radio", "aria-checked": String(k === chosenIcon), title: k, "aria-label": k,
      onclick: (e) => { chosenIcon = k; icons.querySelectorAll(".icon-choice").forEach((b) => { b.classList.toggle("on", b === e.currentTarget); b.setAttribute("aria-checked", String(b === e.currentTarget)); }); } }, icon(k, 18))));

  const browse = h("button.btn.small", { type: "button", onclick: async () => {
    const p = await openDialog({ title: "Choisir une clé privée", defaultPath: "~/.ssh" });
    if (p) f.key_path.value = p;
  } }, "Parcourir…");
  const importBtn = h("button.btn.small", { type: "button", title: "Copier la clé dans le trousseau de RustTerm", onclick: async () => {
    const p = await openDialog({ title: "Importer une clé privée" });
    if (p) try { f.key_path.value = await invoke("import_key", { path: p }); toast("Clé importée", "success"); } catch (e) { toast(String(e), "error"); }
  } }, "Importer…");

  const ok = await modal(isNew ? "Nouvel hôte" : "Modifier l'hôte", (close) => h("form.grid", { onsubmit: (e) => { e.preventDefault(); close(true); } },
    field("Nom", f.name), field("Hôte / IP", f.host),
    field("Utilisateur", f.user), field("Port", f.port),
    h("div.span2", {}, field("Clé privée", h("div.inline", {}, f.key_path, browse, importBtn),
      "L'agent SSH (Bitwarden, 1Password, KeePassXC, Proton Pass…) est toujours essayé en premier.")),
    field("Mot de passe", f.password, "Stocké dans le trousseau du système."), field("Phrase de passe", f.passphrase),
    h("div.span2", {}, field("Description", f.description)),
    field("Tags", f.tags, "Séparés par des virgules"), field("Dossier", f.folder),
    h("div.span2", {}, field("Icône", icons)),
    h("label.check.span2", {}, f.favorite, "Favori (affiché dans la barre des menus / zone de notification)"),
    h("footer.span2", {}, h("button.btn", { type: "button", onclick: () => close(false) }, "Annuler"), h("button.btn.primary", { type: "submit" }, "Enregistrer"))), { wide: true });
  if (!ok) return;

  const port = Number(f.port.value) || 22;
  if (!f.host.value.trim() || port < 1 || port > 65535) return toast("Hôte ou port invalide", "error");
  Object.assign(x, {
    name: f.name.value.trim() || f.host.value.trim(), host: f.host.value.trim(), port, user: f.user.value.trim(),
    key_path: f.key_path.value.trim(), description: f.description.value, icon: chosenIcon,
    tags: f.tags.value.split(",").map((t) => t.trim()).filter(Boolean), folder: f.folder.value || null, favorite: f.favorite.checked,
  });
  try {
    if (f.password.value) await invoke("set_secret", { id: x.id, value: f.password.value });
    if (f.passphrase.value) await invoke("set_secret", { id: x.id + ":passphrase", value: f.passphrase.value });
  } catch (e) { toast("Trousseau : " + e, "error"); }
  const i = S.hosts.hosts.findIndex((y) => y.id === x.id);
  i < 0 ? S.hosts.hosts.push(x) : (S.hosts.hosts[i] = x);
  await saveHosts();
}

/** Export/import as JSON. Secrets stay in the OS keychain and are not exported. */
export async function exportHosts() {
  const path = await saveDialog({ title: "Exporter les hôtes", defaultPath: "rustterm-hosts.json", filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path) return;
  try { await invoke("export_hosts", { path }); toast("Hôtes exportés (sans les mots de passe)", "success"); } catch (e) { toast(String(e), "error"); }
}

export async function importHosts() {
  const path = await openDialog({ title: "Importer des hôtes", filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path) return;
  try {
    const doc = await invoke("read_hosts_file", { path });
    const known = new Set(S.hosts.hosts.map((x) => x.id));
    const kf = new Set(S.hosts.folders.map((x) => x.id));
    S.hosts.folders.push(...doc.folders.filter((x) => !kf.has(x.id)));
    const added = doc.hosts.filter((x) => !known.has(x.id));
    S.hosts.hosts.push(...added);
    await saveHosts();
    toast(`${added.length} hôte(s) importé(s)`, "success");
  } catch (e) { toast(String(e), "error"); }
}
