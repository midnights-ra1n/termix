// Settings → Sync: server or local-network sync, the end-to-end encrypted vault and synced SSH keys.
// Every action goes straight to the backend; nothing here waits for the settings "Save" button.
import { open as openDialog, confirm } from "@tauri-apps/plugin-dialog";
import { h, invoke, field, toast, modal } from "./ui.js";
import { t } from "./i18n.js";

const when = (ms) => (ms ? new Date(ms).toLocaleString() : t("sync_never"));
let shownCode = null; // { code, until }: survives redraws while it is valid

/** Click handler that disables its button while running and reports errors. */
const act = (fn, done) => async (e) => {
  const b = e?.currentTarget;
  if (b) b.disabled = true;
  try {
    await fn();
    if (done) toast(done, "success");
  } catch (x) { toast(t("sync_error", x), "error"); }
  finally { if (b?.isConnected) b.disabled = false; }
};

export function syncView() {
  const root = h("div.sync");
  let last = "";
  const draw = async () => {
    const st = await invoke("sync_status");
    last = JSON.stringify([st.peers, st.discovered, st.last_ok, st.last_error, st.unlocked]);
    const mode = h("select", {}, ...[["off", t("sync_off")], ["server", t("sync_server")], ["p2p", t("sync_p2p")]]
      .map(([v, l]) => h("option", { value: v, selected: v === st.mode }, l)));
    const device = h("input", { value: st.device_name });
    const apply = () => invoke("sync_set_mode", { mode: mode.value, deviceName: device.value }).then(draw, (x) => toast(t("sync_error", x), "error"));
    mode.onchange = apply;
    device.onchange = apply;
    const parts = [h("p.mute.small", {}, t("sync_intro")), h("div.form-grid", {}, field(t("sync_mode"), mode), field(t("sync_device"), device))];
    if (st.mode === "server") parts.push(serverPart(st, draw));
    if (st.mode === "p2p") parts.push(p2pPart(st, draw));
    if (st.mode === "p2p" || (st.mode === "server" && st.logged_in)) parts.push(vaultPart(st, draw), await keysPart(st, draw), statusLine(st, draw));
    root.replaceChildren(...parts);
  };
  draw().catch((x) => root.replaceChildren(h("p.error", {}, t("sync_error", x))));
  // Devices come and go on the network: refresh, but never under the user's typing.
  const timer = setInterval(async () => {
    if (!root.isConnected) { if (!document.querySelector(".settings-modal")) clearInterval(timer); return; }
    if (root.contains(document.activeElement) && document.activeElement.matches("input, select")) return;
    const st = await invoke("sync_status").catch(() => null);
    if (st && JSON.stringify([st.peers, st.discovered, st.last_ok, st.last_error, st.unlocked]) !== last) draw();
  }, 3000);
  return root;
}

function serverPart(st, draw) {
  if (st.logged_in) {
    return h("div.sync-box", {}, h("p", {}, t("sync_signed_in", st.server_url, st.user)),
      h("div.actions", {}, h("button.btn.small", { type: "button", onclick: act(() => invoke("sync_logout").then(draw)) }, t("sync_signout"))));
  }
  const url = h("input", { value: st.server_url, placeholder: "https://sync.example.com", autocomplete: "url" });
  const user = h("input", { value: st.user, autocomplete: "username" });
  const pw = h("input", { type: "password", autocomplete: "current-password" });
  const login = act(() => invoke("sync_login", { url: url.value.trim(), user: user.value.trim(), password: pw.value }).then(draw), t("sync_done"));
  pw.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); login(); } };
  return h("div.sync-box", {},
    h("div.form-grid", {}, h("div.span2", {}, field(t("sync_url"), url)), field(t("sync_user"), user), field(t("sync_password"), pw)),
    h("div.actions", {}, h("button.btn.primary.small", { type: "button", onclick: login }, t("sync_signin"))));
}

function vaultPart(st, draw) {
  if (st.unlocked) {
    return h("div.sync-box", {}, h("h4", {}, t("vault_title")),
      h("div.inline", {}, h("span.grow.small", {}, t("vault_unlocked")),
        h("button.btn.small", { type: "button", onclick: act(() => invoke("sync_lock").then(draw)) }, t("vault_lock"))));
  }
  const pass = h("input", { type: "password", "aria-label": t("vault_pass"), placeholder: t("vault_pass"), autocomplete: st.vault_exists ? "current-password" : "new-password" });
  const unlock = act(() => invoke("sync_unlock", { passphrase: pass.value }).then(draw));
  pass.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); unlock(); } };
  return h("div.sync-box", {}, h("h4", {}, t("vault_title")), h("p.mute.small", {}, st.vault_exists ? t("vault_hint") : t("vault_hint_new")),
    h("div.inline", {}, pass, h("button.btn.primary.small", { type: "button", onclick: unlock }, st.vault_exists ? t("vault_unlock") : t("vault_create"))));
}

async function keysPart(st, draw) {
  const keys = await invoke("sync_keys");
  const add = act(async () => {
    const path = await openDialog({ title: t("sync_key_add"), defaultPath: "~/.ssh" });
    if (!path) return;
    await invoke("sync_key_import", { path, name: path.split(/[\\/]/).pop() });
    await draw();
  });
  const del = (k) => act(async () => {
    if (!(await confirm(t("sync_key_delete", k.name), { kind: "warning" }))) return;
    await invoke("sync_key_delete", { id: k.id });
    await draw();
  });
  return h("div.sync-box", {}, h("h4", {}, t("sync_keys")), h("p.mute.small", {}, st.unlocked ? t("sync_keys_hint") : t("sync_key_locked")),
    h("ul.items", {}, ...(keys.length
      ? keys.map((k) => h("li.item", {}, h("div.item-text", {}, h("strong", {}, k.name || k.id), h("small.item-meta.mono", {}, (k.pub || "").slice(0, 56))),
        h("button.btn.ghost.small", { type: "button", disabled: !st.unlocked, onclick: del(k) }, t("delete").replace(/…$/, ""))))
      : [h("li.mute.small", {}, t("sync_key_none"))])),
    h("div.actions", {}, h("button.btn.small", { type: "button", disabled: !st.unlocked, onclick: add }, t("sync_key_add"))));
}

function askCode(name) {
  return modal(t("p2p_pair"), (close) => {
    const input = h("input", { inputmode: "numeric", maxlength: 6, autofocus: true, autocomplete: "off" });
    return h("form.grid", { onsubmit: (e) => { e.preventDefault(); close(input.value.trim()); } },
      h("div.span2", {}, field(t("p2p_enter_code", name), input)),
      h("footer.span2", {}, h("button.btn", { type: "button", onclick: () => close(null) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("p2p_pair"))));
  });
}

function p2pPart(st, draw) {
  const code = h("p.pair-code", { role: "status" }, shownCode && shownCode.until > Date.now() ? t("p2p_code", shownCode.code) : "");
  const show = act(async () => {
    shownCode = { code: await invoke("p2p_pair_start"), until: Date.now() + 120000 };
    code.textContent = t("p2p_code", shownCode.code);
  });
  const pair = (d) => act(async () => {
    const c = await askCode(d.name);
    if (!c) return;
    toast(t("p2p_paired_ok", await invoke("p2p_pair", { peer: d.id, code: c })), "success");
    await draw();
  });
  const row = (name, meta, button) => h("li.item", {}, h("div.item-text", {}, h("strong", {}, name), meta && h("small.item-meta", {}, meta)), button);
  return h("div.sync-box", {}, h("p.mute.small", {}, t("p2p_hint")),
    st.peers.length ? h("h4", {}, t("p2p_paired")) : null,
    st.peers.length ? h("ul.items", {}, ...st.peers.map((p) => row(p.name, p.online ? t("p2p_online") : t("p2p_offline"),
      h("button.btn.ghost.small", { type: "button", onclick: act(() => invoke("p2p_forget", { peer: p.id }).then(draw)) }, t("p2p_forget"))))) : null,
    h("h4", {}, t("p2p_found")),
    h("ul.items", {}, ...(st.discovered.length
      ? st.discovered.map((d) => row(d.name, null, h("button.btn.small", { type: "button", onclick: pair(d) }, t("p2p_pair"))))
      : [h("li.mute.small", {}, t("p2p_none"))])),
    h("div.actions", {}, h("button.btn.small", { type: "button", onclick: show }, t("p2p_show_code"))), code);
}

function statusLine(st, draw) {
  return h("div.sync-status", {},
    h("span.grow.small" + (st.last_error ? ".error" : ".mute"), {}, st.last_error ? t("sync_error", st.last_error) : t("sync_last", when(st.last_ok))),
    h("button.btn.small", { type: "button", onclick: act(() => invoke("sync_now").then(draw), t("sync_done")) }, t("sync_now")));
}
