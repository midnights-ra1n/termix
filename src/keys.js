// Settings → SSH keys: every key Termix holds, kept on this device only or synced through the encrypted vault.
// Actions go straight to the backend, like Settings → Sync.
import { open as openDialog, confirm } from "@tauri-apps/plugin-dialog";
import { h, invoke, field, toast, modal } from "./ui.js";
import { t } from "./i18n.js";

const kind = (k) => (k.pub || "").split(" ")[0].replace(/^ssh-/, "") || t("key_encrypted");

/** Adds a key from pasted text or a file. `sync` defaults to the vault when it is open on this device. Returns its id. */
export async function importKey({ text = null, path = null, name = "", sync = null }) {
  if (sync === null) sync = (await invoke("sync_status").catch(() => ({}))).unlocked === true;
  const id = await invoke("key_import", { text, path, name: name || (path ? path.split(/[\\/]/).pop() : ""), sync });
  toast(t(sync ? "key_imported_synced" : "key_imported_local"), "success");
  return id;
}

/** Paste dialog: the key's text, a name and where to keep it. Resolves to the new key's id, or null. */
export async function pasteKey(canSync) {
  const text = h("textarea.mono", { rows: 8, placeholder: "-----BEGIN OPENSSH PRIVATE KEY-----", "aria-label": t("key_text"), autofocus: true });
  const name = h("input", { placeholder: t("key_name_ph") });
  const sync = h("input.switch", { type: "checkbox", role: "switch", checked: canSync, disabled: !canSync });
  const ok = await modal(t("key_paste"), (close) => h("form.grid", { onsubmit: (e) => { e.preventDefault(); close(true); } },
    h("div.span2", {}, field(t("key_text"), text)), h("div.span2", {}, field(t("sync_key_name"), name)),
    h("label.toggle.span2", {}, h("span", {}, t("key_sync_toggle"), h("small", {}, t(canSync ? "key_sync_hint" : "keys_sync_off"))), sync),
    h("footer.span2", {}, h("button.btn", { type: "button", onclick: () => close(false) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("import")))), { wide: true });
  if (!ok || !text.value.trim()) return null;
  try { return await importKey({ text: text.value, name: name.value.trim(), sync: sync.checked }); } catch (e) { toast(String(e), "error"); return null; }
}

export function keysView() {
  const root = h("div.keys");
  const draw = async () => {
    const [keys, st] = await Promise.all([invoke("keys"), invoke("sync_status")]);
    const canSync = st.unlocked === true;
    /** Click handler: runs, redraws, reports errors. */
    const run = (fn) => async (e) => {
      const b = e?.currentTarget;
      if (b) b.disabled = true;
      try { await fn(); } catch (x) { toast(String(x), "error"); }
      await draw();
    };
    const move = (list, synced) => run(async () => {
      if (!list.length) return;
      const msg = synced ? t("keys_sync_confirm", list.length) : t("keys_unsync_confirm", list.length);
      if (!(await confirm(msg, { kind: synced ? "info" : "warning" }))) return;
      await invoke("keys_set_synced", { ids: list.map((k) => k.id), synced });
    });
    const row = (k) => {
      const sw = h("input.switch", { type: "checkbox", role: "switch", checked: k.synced, disabled: !canSync && !k.synced, "aria-label": t("key_sync_toggle") + " — " + k.name });
      sw.onchange = () => { sw.checked = k.synced; move([k], !k.synced)(); };
      return h("li.item", {},
        h("div.item-text", {}, h("strong.item-name", {}, k.name),
          h("small.item-meta.mono", { title: k.pub || "" }, `${kind(k)} · ${t(k.synced ? "key_where_synced" : "key_where_local")}`)),
        h("label.inline.small", { title: t(canSync ? "key_sync_hint" : "keys_sync_off") }, h("span.mute", {}, t("key_synced")), sw),
        k.pub ? h("button.btn.ghost.small", { type: "button", onclick: () => navigator.clipboard.writeText(k.pub).then(() => toast(t("key_pub_copied"), "success")) }, t("key_copy_pub")) : null,
        h("button.btn.ghost.small", { type: "button", onclick: run(async () => {
          if (!(await confirm(t(k.synced ? "sync_key_delete" : "key_delete_local", k.name), { kind: "warning" }))) return;
          await invoke("key_delete", { id: k.id });
        }) }, t("delete").replace(/…$/, "")));
    };
    const local = keys.filter((k) => !k.synced), synced = keys.filter((k) => k.synced);
    root.replaceChildren(
      h("p.mute.small", {}, t("keys_intro")),
      h("div.actions", {},
        h("button.btn.small", { type: "button", onclick: run(async () => {
          const path = await openDialog({ title: t("import_key"), defaultPath: "~/.ssh" });
          if (path) await importKey({ path, sync: canSync });
        }) }, t("key_import_file")),
        h("button.btn.small", { type: "button", onclick: run(() => pasteKey(canSync)) }, t("key_paste"))),
      h("ul.items", {}, ...(keys.length ? keys.map(row) : [h("li.mute.small", {}, t("sync_key_none"))])),
      keys.length ? h("div.sync-box", {}, h("h4", {}, t("keys_all")),
        h("p.mute.small", {}, t(canSync ? "keys_all_hint" : "keys_sync_off")),
        h("div.actions", {},
          h("button.btn.small", { type: "button", disabled: !canSync || !local.length, onclick: move(local, true) }, t("keys_sync_all", local.length)),
          h("button.btn.small", { type: "button", disabled: !synced.length, onclick: move(synced, false) }, t("keys_unsync_all", synced.length)))) : null);
  };
  draw().catch((e) => root.replaceChildren(h("p.error", {}, String(e))));
  return root;
}
