// Termix sync web UI. Secrets and SSH keys are decrypted here, in the browser, with the account
// passphrase: the server only ever stores and serves ciphertext.
// Format (same as the app): AES-256-GCM, base64(nonce12 ‖ ciphertext ‖ tag), key = PBKDF2-SHA256(600 000).
const ROUNDS = 600000;
const CHECK = "termix-vault";
const DEV = "web";

const S = { token: sessionStorage.getItem("token"), me: null, records: new Map(), vault: null, view: "hosts" };

// ---------- DOM ----------
function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k in el && typeof v !== "string") el[k] = v;
    else el.setAttribute(k, v === true ? "" : v);
  }
  for (const c of kids.flat()) if (c != null && c !== false) el.append(c instanceof Node ? c : String(c));
  return el;
}
const app = () => document.getElementById("app");
// crypto.randomUUID only exists on HTTPS or localhost; the server is often opened over plain HTTP on a LAN.
const uuid = () => crypto.randomUUID?.() ?? "10000000-1000-4000-8000-100000000000".replace(/[018]/g, (c) => (c ^ (crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (c / 4)))).toString(16));
const fmtDate = (ms) => (ms ? new Date(ms).toLocaleDateString(undefined, { dateStyle: "medium" }) : "");
const fmtTime = (ms) => (ms ? new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "");
/** Page title, one-line description and the page's main action on the same line. */
const head = (title, lead, action) => h("div", { class: "head" }, h("div", {}, h("h1", {}, title), h("p", { class: "lead" }, lead)), action);

// ---------- API ----------
async function api(method, path, body) {
  const r = await fetch(path, {
    method,
    headers: { "content-type": "application/json", ...(S.token ? { authorization: `Bearer ${S.token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (r.status === 401 && path !== "/api/login") { signOut(); throw new Error("Session expired"); }
  if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `${r.status} ${r.statusText}`);
  return r.status === 204 ? null : r.json();
}

async function pull() {
  const { records } = await api("GET", "/api/records?after=0");
  S.records = new Map(records.map((r) => [r.key, r]));
}

async function put(key, v) {
  const rec = { key, v, ts: Date.now(), dev: DEV };
  await api("POST", "/api/records", { records: [rec] });
  S.records.set(key, rec);
}

const live = (prefix) => [...S.records.values()].filter((r) => r.key.startsWith(prefix) && r.v != null);

// ---------- vault ----------
const b64 = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const unb64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
const enc = new TextEncoder();
const dec = new TextDecoder();

async function deriveKey(pass, saltB64) {
  if (!crypto.subtle) throw new Error("The vault needs HTTPS (or localhost): browsers turn encryption off on plain HTTP");
  const base = await crypto.subtle.importKey("raw", enc.encode(pass), "PBKDF2", false, ["deriveKey"]);
  return crypto.subtle.deriveKey({ name: "PBKDF2", hash: "SHA-256", salt: unb64(saltB64), iterations: ROUNDS }, base,
    { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}
async function seal(text) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, S.vault, enc.encode(text));
  const out = new Uint8Array(12 + ct.byteLength);
  out.set(iv); out.set(new Uint8Array(ct), 12);
  return b64(out);
}
async function open(sealed, key = S.vault) {
  const b = unb64(sealed);
  return dec.decode(await crypto.subtle.decrypt({ name: "AES-GCM", iv: b.slice(0, 12) }, key, b.slice(12)));
}

async function unlock(pass) {
  const key = await deriveKey(pass, S.me.salt);
  const check = S.records.get("vault:check");
  if (check?.v?.enc) {
    const ok = await open(check.v.enc, key).then((t) => t === CHECK, () => false);
    if (!ok) throw new Error("Wrong passphrase");
    S.vault = key;
  } else {
    S.vault = key;
    await put("vault:check", { enc: await seal(CHECK) });
  }
}

function vaultCard(onDone) {
  if (S.vault) return null;
  const err = h("p", { class: "error" });
  const pass = h("input", { type: "password", autocomplete: "current-password", required: true, minLength: 8 });
  const fresh = !S.records.get("vault:check")?.v;
  return h("form", { class: "card", onsubmit: async (e) => {
      e.preventDefault(); err.textContent = "Unlocking…";
      try { await unlock(pass.value); onDone(); } catch (x) { err.textContent = x.message; }
    } },
    h("h2", {}, fresh ? "Create the vault" : "Unlock the vault"),
    h("p", { class: "muted" }, fresh
      ? "Choose the passphrase that encrypts passwords and SSH keys for this account. Every device will need it. It never leaves your browser and cannot be recovered."
      : "Passwords and SSH keys are end-to-end encrypted. Enter the account passphrase to read or change them. It stays in this tab."),
    h("label", {}, "Passphrase", pass),
    h("div", { class: "row" }, h("button", { class: "btn primary" }, fresh ? "Create" : "Unlock")), err);
}

// ---------- shell ----------
function signOut() {
  if (S.token) fetch("/api/logout", { method: "POST", headers: { authorization: `Bearer ${S.token}` } }).catch(() => {});
  sessionStorage.removeItem("token");
  Object.assign(S, { token: null, me: null, vault: null, records: new Map() });
  render();
}

function loginView() {
  const err = h("p", { class: "error" });
  const user = h("input", { autocomplete: "username", required: true });
  const pw = h("input", { type: "password", autocomplete: "current-password", required: true });
  return h("div", { class: "login" }, h("form", { onsubmit: async (e) => {
      e.preventDefault(); err.textContent = "";
      try {
        const r = await api("POST", "/api/login", { user: user.value.trim(), password: pw.value, device: "Web browser" });
        S.token = r.token; sessionStorage.setItem("token", r.token);
        await boot();
      } catch (x) { err.textContent = x.message; }
    } },
    h("div", { class: "brand" }, h("img", { src: "/logo.svg", alt: "" }), "Termix sync"),
    h("p", { class: "lead" }, "Hosts, settings and keys of your Termix devices"),
    h("label", {}, "User", user), h("label", {}, "Password", pw),
    h("button", { class: "btn primary" }, "Sign in"), err));
}

const VIEWS = { hosts: ["Hosts", hostsView], keys: ["SSH keys", keysView], account: ["Account", accountView], users: ["Users", usersView] };

function render() {
  if (!S.token || !S.me) return app().replaceChildren(loginView());
  const nav = h("nav", {}, h("div", { class: "brand" }, h("img", { src: "/logo.svg", alt: "" }), "Termix sync"),
    Object.entries(VIEWS).filter(([k]) => k !== "users" || S.me.admin).map(([k, [label]]) =>
      h("button", { "aria-current": S.view === k ? "page" : null, onclick: () => { S.view = k; render(); } }, label)),
    h("div", { class: "who" }, h("span", {}, S.me.user), h("button", { onclick: signOut }, "Sign out")));
  const main = h("section", {});
  app().replaceChildren(h("div", { class: "shell" }, nav, main));
  VIEWS[S.view][1](main).catch((x) => main.append(h("p", { class: "error" }, x.message)));
}

async function boot() {
  try { S.me = await api("GET", "/api/me"); await pull(); } catch { S.me = null; }
  render();
}

// ---------- hosts ----------
async function hostsView(main) {
  await pull();
  const hosts = live("host:").map((r) => r.v).sort((a, b) => (a.name || "").localeCompare(b.name || ""));
  const folders = Object.fromEntries(live("folder:").map((r) => [r.v.id, r.v.name]));
  main.append(head("Hosts", "Saved hosts of your account. Changes reach your devices on their next sync",
    h("button", { class: "btn primary", onclick: () => hostDialog() }, "Add host")));
  if (!hosts.length) return main.append(h("p", { class: "empty" }, "No host yet. Add one here or turn sync on in the Termix app."));
  main.append(h("table", {},
    h("thead", {}, h("tr", {}, h("th", {}, "Name"), h("th", {}, "Address"), h("th", { class: "hide-sm" }, "Folder"), h("th", { class: "hide-sm" }, "Tags"), h("th", {}))),
    h("tbody", {}, hosts.map((x) => h("tr", {},
      h("td", {}, x.name || x.host),
      h("td", { class: "mono" }, `${x.user ? x.user + "@" : ""}${x.host}${x.port && x.port !== 22 ? ":" + x.port : ""}`),
      h("td", { class: "hide-sm muted" }, folders[x.folder] || ""),
      h("td", { class: "hide-sm" }, (x.tags || []).map((t) => h("span", { class: "tag" }, t))),
      h("td", { class: "actions" },
        h("button", { class: "btn small", onclick: () => hostDialog(x) }, "Edit"), " ",
        h("button", { class: "btn small danger", onclick: async () => {
          if (!confirm(`Delete ${x.name || x.host} from every device?`)) return;
          await put(`host:${x.id}`, null);
          for (const k of [`secret:${x.id}`, `secret:${x.id}:passphrase`]) if (S.records.get(k)?.v) await put(k, null);
          render();
        } }, "Delete")))))));
}

function hostDialog(x) {
  const isNew = !x;
  x = structuredClone(x || { id: uuid(), name: "", host: "", port: 22, user: "", key_path: "", key_id: "", description: "", icon: "server", tags: [], folder: null, favorite: false });
  const f = (k, attrs = {}) => h("input", { value: x[k] ?? "", ...attrs });
  const name = f("name"), host = f("host", { required: true }), port = f("port", { type: "number", min: 1, max: 65535 }), user = f("user");
  const desc = f("description"), tags = h("input", { value: (x.tags || []).join(", ") });
  const folder = h("select", {}, h("option", { value: "" }, "None"),
    live("folder:").map((r) => h("option", { value: r.v.id, selected: r.v.id === x.folder }, r.v.name)));
  const key = h("select", {}, h("option", { value: "" }, "Agent or password"),
    live("key:").map((r) => h("option", { value: r.key.slice(4), selected: r.key.slice(4) === x.key_id }, r.v.name || r.key.slice(4))));
  const pw = h("input", { type: "password", autocomplete: "new-password", placeholder: S.vault ? (S.records.get(`secret:${x.id}`)?.v ? "Saved, leave empty to keep" : "") : "Unlock the vault to set it", disabled: !S.vault });
  const err = h("p", { class: "error" });
  const dlg = h("dialog", {}, h("form", { method: "dialog", onsubmit: async (e) => {
      e.preventDefault();
      Object.assign(x, { name: name.value.trim(), host: host.value.trim(), port: Number(port.value) || 22, user: user.value.trim(),
        description: desc.value, tags: tags.value.split(",").map((t) => t.trim()).filter(Boolean), folder: folder.value || null, key_id: key.value });
      try {
        await put(`host:${x.id}`, x);
        if (pw.value) await put(`secret:${x.id}`, { enc: await seal(pw.value) });
        dlg.close(); render();
      } catch (y) { err.textContent = y.message; }
    } },
    h("h1", {}, isNew ? "Add host" : "Edit host"),
    h("label", {}, "Name", name),
    h("div", { class: "grid2" }, h("label", {}, "Address", host), h("label", {}, "Port", port)),
    h("div", { class: "grid2" }, h("label", {}, "User", user), h("label", {}, "Folder", folder)),
    h("label", {}, "SSH key", key), h("label", {}, "Password", pw),
    h("label", {}, "Tags, separated by commas", tags), h("label", {}, "Description", desc), err,
    h("div", { class: "row" }, h("button", { class: "btn primary" }, "Save"), h("button", { class: "btn", type: "button", onclick: () => dlg.close() }, "Cancel"))));
  dlg.addEventListener("close", () => dlg.remove());
  document.body.append(dlg); dlg.showModal();
}

// ---------- SSH keys ----------
async function keysView(main) {
  await pull();
  const lock = vaultCard(render);
  main.append(head("SSH keys", "Private keys encrypted in your vault, used by the hosts that select them on every device",
    lock ? null : h("button", { class: "btn primary", onclick: keyDialog }, "Add key")));
  if (lock) return main.append(lock);
  const keys = live("key:");
  if (!keys.length) return main.append(h("p", { class: "empty" }, "No key yet."));
  main.append(h("table", {}, h("thead", {}, h("tr", {}, h("th", {}, "Name"), h("th", { class: "hide-sm" }, "Public key"), h("th", {}))),
    h("tbody", {}, keys.map((r) => h("tr", {},
      h("td", {}, r.v.name || r.key.slice(4)),
      h("td", { class: "hide-sm mono muted" }, (r.v.pub || "").slice(0, 60) + ((r.v.pub || "").length > 60 ? "…" : "")),
      h("td", { class: "actions" },
        r.v.pub ? h("button", { class: "btn small", onclick: () => navigator.clipboard.writeText(r.v.pub) }, "Copy public key") : null, " ",
        h("button", { class: "btn small danger", onclick: async () => {
          if (!confirm(`Delete the key ${r.v.name} from every device?`)) return;
          await put(r.key, null); render();
        } }, "Delete")))))));
}

function keyDialog() {
  const name = h("input", { required: true }), pem = h("textarea", { required: true, spellcheck: false, placeholder: "-----BEGIN OPENSSH PRIVATE KEY-----" });
  const pub = h("input", { placeholder: "ssh-ed25519 AAAA… (optional)" });
  const file = h("input", { type: "file", onchange: async () => { pem.value = await file.files[0].text(); if (!name.value) name.value = file.files[0].name; } });
  const err = h("p", { class: "error" });
  const dlg = h("dialog", {}, h("form", { method: "dialog", onsubmit: async (e) => {
      e.preventDefault();
      if (!pem.value.includes("PRIVATE KEY")) return (err.textContent = "This is not a private key");
      try {
        await put(`key:${uuid()}`, { name: name.value.trim(), pub: pub.value.trim(), enc: await seal(pem.value.trim() + "\n") });
        dlg.close(); render();
      } catch (y) { err.textContent = y.message; }
    } },
    h("h1", {}, "Add key"), h("label", {}, "Name", name), h("label", {}, "Private key file", file), h("label", {}, "Or paste it", pem),
    h("label", {}, "Public key", pub), err,
    h("div", { class: "row" }, h("button", { class: "btn primary" }, "Encrypt and save"), h("button", { class: "btn", type: "button", onclick: () => dlg.close() }, "Cancel"))));
  dlg.addEventListener("close", () => dlg.remove());
  document.body.append(dlg); dlg.showModal();
}

// ---------- account ----------
async function accountView(main) {
  S.me = await api("GET", "/api/me");
  const old = h("input", { type: "password", autocomplete: "current-password", required: true });
  const neu = h("input", { type: "password", autocomplete: "new-password", required: true, minLength: 8 });
  const msg = h("p", { class: "error" });
  main.append(head("Account", `Signed in as ${S.me.user}`),
    h("form", { class: "card", onsubmit: async (e) => {
        e.preventDefault();
        try { await api("POST", "/api/me/password", { old: old.value, new: neu.value }); msg.textContent = "Password changed"; old.value = neu.value = ""; }
        catch (x) { msg.textContent = x.message; }
      } },
      h("h2", {}, "Sign-in password"),
      h("p", { class: "muted" }, "This password only signs you in. The vault passphrase is separate and is never sent here."),
      h("div", { class: "grid2" }, h("label", {}, "Current", old), h("label", {}, "New", neu)),
      h("div", { class: "row" }, h("button", { class: "btn primary" }, "Change password")), msg),
    h("div", { class: "card" }, h("h2", {}, "Signed-in devices"),
      h("table", {}, h("tbody", {}, S.me.devices.map((d) => h("tr", {},
        h("td", {}, d.device, d.current ? h("span", { class: "muted" }, " · this browser") : null),
        h("td", { class: "muted hide-sm" }, `Last seen ${fmtTime(d.last_used)}`),
        h("td", { class: "actions" }, d.current ? null : h("button", { class: "btn small danger", onclick: async () => {
          await api("DELETE", `/api/devices/${d.id}`); render();
        } }, "Sign out"))))))));
}

// ---------- users (admin) ----------
async function usersView(main) {
  const users = await api("GET", "/api/users");
  main.append(head("Users", "Each user has their own hosts, settings and vault. Administrators cannot read anyone's secrets",
    h("button", { class: "btn primary", onclick: userDialog }, "Add user")),
    h("table", {}, h("thead", {}, h("tr", {}, h("th", {}, "User"), h("th", { class: "hide-sm" }, "Role"), h("th", {}, "Hosts"), h("th", { class: "hide-sm" }, "Devices"), h("th", { class: "hide-sm" }, "Created"), h("th", {}))),
      h("tbody", {}, users.map((u) => h("tr", {},
        h("td", {}, u.name), h("td", { class: "hide-sm role" }, u.admin ? "Administrator" : "Member"),
        h("td", {}, u.hosts), h("td", { class: "hide-sm" }, u.devices), h("td", { class: "hide-sm muted" }, fmtDate(u.created)),
        h("td", { class: "actions" },
          h("button", { class: "btn small", onclick: async () => {
            const pw = prompt(`New password for ${u.name} (8 characters minimum). Their devices will be signed out.`);
            if (pw) await api("PATCH", `/api/users/${u.id}`, { password: pw }).then(render, (x) => alert(x.message));
          } }, "Reset password"), " ",
          h("button", { class: "btn small", onclick: () => api("PATCH", `/api/users/${u.id}`, { admin: !u.admin }).then(render, (x) => alert(x.message)) },
            u.admin ? "Remove admin" : "Make admin"), " ",
          u.name === S.me.user ? null : h("button", { class: "btn small danger", onclick: async () => {
            if (confirm(`Delete ${u.name} and all their synced data?`)) await api("DELETE", `/api/users/${u.id}`).then(render, (x) => alert(x.message));
          } }, "Delete")))))));
}

function userDialog() {
  const name = h("input", { required: true, pattern: "[A-Za-z0-9._\\-]{1,64}", autocomplete: "off" });
  const pw = h("input", { type: "password", required: true, minLength: 8, autocomplete: "new-password" });
  const adm = h("input", { type: "checkbox" });
  const err = h("p", { class: "error" });
  const dlg = h("dialog", {}, h("form", { method: "dialog", onsubmit: async (e) => {
      e.preventDefault();
      try { await api("POST", "/api/users", { name: name.value.trim(), password: pw.value, admin: adm.checked }); dlg.close(); render(); }
      catch (x) { err.textContent = x.message; }
    } },
    h("h1", {}, "Add user"), h("label", {}, "User name", name), h("label", {}, "Password", pw),
    h("label", { class: "check" }, adm, "Administrator"), err,
    h("div", { class: "row" }, h("button", { class: "btn primary" }, "Create"), h("button", { class: "btn", type: "button", onclick: () => dlg.close() }, "Cancel"))));
  dlg.addEventListener("close", () => dlg.remove());
  document.body.append(dlg); dlg.showModal();
}

boot();
