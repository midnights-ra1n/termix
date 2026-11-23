// Settings → Sync: server or local-network sync and the end-to-end encrypted vault (keys: Settings → SSH keys).
// Every action goes straight to the backend; nothing here waits for the settings "Save" button.
import { listen } from "@tauri-apps/api/event";
import { h, icon, invoke, field, toast, modal } from "./ui.js";
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
    last = snapshot(st);
    const mode = h("select", {}, ...[["off", t("sync_off")], ["server", t("sync_server")], ["p2p", t("sync_p2p")]]
      .map(([v, l]) => h("option", { value: v, selected: v === st.mode }, l)));
    const device = h("input", { value: st.device_name });
    const apply = () => invoke("sync_set_mode", { mode: mode.value, deviceName: device.value }).then(draw, (x) => toast(t("sync_error", x), "error"));
    mode.onchange = apply;
    device.onchange = apply;
    const every = h("select", { onchange: () => invoke("sync_set_interval", { secs: +every.value }).then(draw, (x) => toast(t("sync_error", x), "error")) },
      ...[...new Set([5, 15, 30, 60, 300, 900, 0, st.interval])].map((v) => h("option", { value: v, selected: v === st.interval }, every_label(v))));
    const keys = h("select", { onchange: () => invoke("sync_set_keybindings", { mode: keys.value }).then(draw, (x) => toast(t("sync_error", x), "error")) },
      ...[["synced", t("sync_keys_synced")], ["local", t("sync_keys_local")]].map(([v, l]) => h("option", { value: v, selected: v === st.keybindings }, l)));
    const take = h("button.btn.small", { type: "button", onclick: act(() => invoke("sync_take_keybindings").then(draw), t("sync_done")) }, t("sync_keys_take"));
    const parts = [h("p.mute.small", {}, t("sync_intro")), h("div.form-grid", {}, field(t("sync_mode"), mode), field(t("sync_device"), device),
      st.mode !== "off" ? h("div.span2", {}, field(t("sync_every"), every, t("sync_every_hint"))) : null,
      st.mode !== "off" ? h("div.span2", {}, field(t("sync_kb"), keys, t(st.keybindings === "local" ? "sync_keys_local_hint" : "sync_kb_hint"))) : null,
      st.mode !== "off" && st.keybindings === "local" ? h("div.span2.actions", {}, take) : null)];
    if (st.conflicts.length) parts.push(conflictsPart(st, draw));
    if (st.mode === "server") parts.push(serverPart(st, draw));
    if (st.mode === "p2p") parts.push(p2pPart(st, draw));
    if (st.mode === "p2p" || (st.mode === "server" && st.logged_in)) parts.push(vaultPart(st, draw), statusLine(st, draw));
    root.replaceChildren(...parts);
  };
  draw().catch((x) => root.replaceChildren(h("p.error", {}, t("sync_error", x))));
  // Devices come and go on the network: refresh, but never under the user's typing.
  const timer = setInterval(async () => {
    if (!root.isConnected) { if (!document.querySelector(".settings-modal")) clearInterval(timer); return; }
    if (root.contains(document.activeElement) && document.activeElement.matches("input, select")) return;
    const st = await invoke("sync_status").catch(() => null);
    if (st && snapshot(st) !== last) draw();
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

const snapshot = (st) => JSON.stringify([st.peers, st.discovered, st.last_ok, st.last_error, st.unlocked, st.conflicts, st.interval]);
const every_label = (s) => (s === 0 ? t("sync_manual") : s <= 5 ? t("sync_live") : s < 60 ? t("sync_secs", s) : t("sync_mins", s / 60));

/** Short, readable differences between two versions of a record (nested settings flattened two levels). */
function diff(a, b) {
  const flat = (v, p = "", d = 0, out = {}) => {
    if (v && typeof v === "object" && !Array.isArray(v) && d < 2) for (const [k, x] of Object.entries(v)) flat(x, p ? `${p}.${k}` : k, d + 1, out);
    else out[p] = v;
    return out;
  };
  if (a == null || b == null) return [];
  const fa = flat(a), fb = flat(b), show = (v) => (typeof v === "string" ? v : JSON.stringify(v)) ?? "—";
  return [...new Set([...Object.keys(fa), ...Object.keys(fb)])].filter((k) => JSON.stringify(fa[k]) !== JSON.stringify(fb[k]))
    .slice(0, 6).map((k) => `${k} : ${show(fa[k])} → ${show(fb[k])}`);
}

function conflictsPart(st, draw) {
  const label = (c) => {
    const v = c.mine.v || c.theirs.v || {};
    return c.key === "settings" ? t("settings_title") : `${c.key.startsWith("folder:") ? t("f_folder") : t("sync_item_host")} « ${v.name || v.host || c.key} »`;
  };
  const side = (r) => (r.v == null ? t("sync_deleted") : when(r.ts));
  return h("div.sync-box", {}, h("h4", {}, t("sync_conflicts")), h("p.mute.small", {}, t("sync_conflicts_hint")),
    h("ul.items", {}, ...st.conflicts.map((c) => h("li.item", {},
      h("div.item-text", {}, h("strong", {}, label(c)),
        h("small.item-meta", {}, `${t("sync_this_device")} : ${side(c.mine)} · ${c.theirs.device} : ${side(c.theirs)}`),
        h("ul.conflict-diff", {}, ...diff(c.mine.v, c.theirs.v).map((d) => h("li", { title: d }, d)))),
      h("div.inline", {},
        h("button.btn.small", { type: "button", onclick: act(() => invoke("sync_resolve", { key: c.key, keep: "mine" }).then(draw)) }, t("sync_keep_mine")),
        h("button.btn.small", { type: "button", onclick: act(() => invoke("sync_resolve", { key: c.key, keep: "theirs" }).then(draw)) }, t("sync_keep_theirs", c.theirs.device)))))));
}

/** Title bar indicator: hidden when sync is off; click syncs now, or opens Settings → Sync on a problem. */
export function initSyncStatus(btn, openSync) {
  let st = null;
  const render = (s) => {
    st = s;
    btn.hidden = s.mode === "off" || !s.mode;
    if (btn.hidden) return;
    const [name, cls, text] = s.conflicts.length ? ["syncConflict", "warn", t("sync_n_conflicts", s.conflicts.length)]
      : s.syncing ? ["syncing", "", t("sync_running")]
      : s.last_error ? ["syncOff", "error", t("sync_error", s.last_error)]
      : ["syncOk", "", t("sync_last", when(s.last_ok))];
    btn.className = "icon-btn " + cls;
    btn.replaceChildren(icon(name, 16));
    btn.title = text;
    btn.setAttribute("aria-label", text);
  };
  btn.onclick = () => (st && (st.conflicts.length || st.last_error) ? openSync() : invoke("sync_now").catch(() => {}));
  listen("sync-status", (e) => render(e.payload));
  invoke("sync_status").then(render, () => {});
}
