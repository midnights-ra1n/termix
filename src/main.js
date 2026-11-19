import "@xterm/xterm/css/xterm.css";
import "./style.css";
import { listen } from "@tauri-apps/api/event";
import { S, h, icon, invoke, modal, field, toast, isMac } from "./ui.js";
import { THEMES, resolveTheme, applyTheme, xtermTheme } from "./themes.js";
import { initHosts, editHost, editFolder, exportHosts, importHosts } from "./hosts.js";
import { initPanes, openHost, split, closePane, cycleFocus, cycleTab, setZoom, applyTerminalSettings, newTab } from "./panes.js";
import { initPanel, refreshPanel } from "./panel.js";

const ACTIONS = {
  new_host: ["Nouvel hôte", () => editHost()],
  new_folder: ["Nouveau dossier", () => editFolder()],
  close_pane: ["Fermer le panneau", closePane],
  split_right: ["Diviser à droite", () => split("right")],
  split_left: ["Diviser à gauche", () => split("left")],
  split_down: ["Diviser en bas", () => split("down")],
  split_up: ["Diviser en haut", () => split("up")],
  focus_next: ["Panneau suivant", () => cycleFocus(1)],
  focus_prev: ["Panneau précédent", () => cycleFocus(-1)],
  next_tab: ["Onglet suivant", () => cycleTab(1)],
  prev_tab: ["Onglet précédent", () => cycleTab(-1)],
  search_hosts: ["Rechercher un hôte", () => { setLayout("sidebar", true); document.getElementById("host-search").focus(); }],
  toggle_sidebar: ["Afficher/masquer les hôtes", () => setLayout("sidebar")],
  toggle_panel: ["Afficher/masquer SFTP et stats", () => setLayout("right_panel")],
  settings: ["Réglages", openSettings],
  zoom_in: ["Agrandir le texte", () => setZoom(1)],
  zoom_out: ["Réduire le texte", () => setZoom(-1)],
  zoom_reset: ["Taille réelle", () => setZoom(0)],
  import_hosts: [null, importHosts],
  export_hosts: [null, exportHosts],
  check_update: [null, () => checkUpdate(true)],
  open_docs: [null, () => toast(`Configuration : ${S.configDir}/config.toml — voir docs/CONFIG.md`)],
};

// ---------- keyboard ----------

const CODE_KEYS = { BracketLeft: "[", BracketRight: "]", Equal: "=", Minus: "-", Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", Backquote: "`" };
function eventKey(e) {
  if (e.key.startsWith("Arrow")) return e.key.slice(5);
  if (/^Key[A-Z]$|^Digit\d$/.test(e.code) && e.key.length !== 1) return e.code.slice(-1);
  return e.key.length === 1 ? e.key.toUpperCase() : e.key;
}
/** Accelerator from an event, e.g. "Cmd+Shift+D" (macOS) / "Ctrl+Alt+D". */
export function accelFromEvent(e) {
  const mods = [];
  if (e.metaKey) mods.push(isMac() ? "Cmd" : "Super");
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  return [...mods, eventKey(e)].join("+");
}
function matches(accel, e) {
  if (!accel) return false;
  const parts = accel.split("+");
  const key = parts.pop().toUpperCase();
  const m = new Set(parts.map((p) => p.toLowerCase()));
  const want = {
    meta: m.has("cmd") || m.has("super") || m.has("command") || (isMac() && m.has("cmdorctrl")),
    ctrl: m.has("ctrl") || m.has("control") || (!isMac() && m.has("cmdorctrl")),
    alt: m.has("alt") || m.has("option"),
    shift: m.has("shift"),
  };
  if (want.meta !== e.metaKey || want.ctrl !== e.ctrlKey || want.alt !== e.altKey || want.shift !== e.shiftKey) return false;
  // Layout-aware letter (AZERTY…), with the physical key as fallback when Shift/Option change the character.
  return eventKey(e).toUpperCase() === key || CODE_KEYS[e.code] === key || e.code === "Key" + key || e.code === "Digit" + key;
}
export function prettyAccel(a) {
  if (!a) return "—";
  if (!isMac()) return a.replace("CmdOrCtrl", "Ctrl");
  const sym = { cmd: "⌘", cmdorctrl: "⌘", command: "⌘", ctrl: "⌃", control: "⌃", alt: "⌥", option: "⌥", shift: "⇧" };
  return a.split("+").map((p) => sym[p.toLowerCase()] || p).join("");
}

// On macOS the native menu owns the shortcuts; elsewhere we catch them before xterm does.
addEventListener("keydown", (e) => {
  if (isMac() || document.querySelector(".backdrop")) return;
  for (const [action, accel] of Object.entries(S.settings.keybindings)) {
    if (ACTIONS[action] && matches(accel, e)) { e.preventDefault(); e.stopPropagation(); ACTIONS[action][1](); return; }
  }
}, true);

// ---------- layout / theme ----------

function setLayout(key, value) {
  const a = S.settings.appearance;
  a[key] = value ?? !a[key];
  applyLayout();
  invoke("save_settings", { settings: S.settings }).catch(() => {});
}
function applyLayout() {
  const a = S.settings.appearance;
  document.body.classList.toggle("no-sidebar", !a.sidebar);
  document.body.classList.toggle("no-panel", !a.right_panel);
  refreshPanel();
}

function applyAppearance() {
  const a = S.settings.appearance;
  const t = resolveTheme(a.theme, S.settings.themes);
  applyTheme(t);
  const r = document.documentElement.style;
  r.setProperty("--ui-font", a.ui_font);
  r.setProperty("--ui-size", a.ui_font_size + "px");
  applyTerminalSettings(xtermTheme(t));
  applyLayout();
}
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => S.settings?.appearance.theme === "system" && applyAppearance());

// ---------- settings ----------

async function openSettings() {
  const s = structuredClone(S.settings);
  const sel = (value, opts) => h("select", {}, ...opts.map(([v, l]) => h("option", { value: v, selected: v === value }, l)));
  const allThemes = { ...THEMES, ...s.themes };
  const f = {
    theme: sel(s.appearance.theme, Object.entries(allThemes).map(([k, t]) => [k, t.label || k])),
    ui_font: h("input", { value: s.appearance.ui_font }),
    ui_font_size: h("input", { type: "number", min: 10, max: 24, value: s.appearance.ui_font_size }),
    font_family: h("input", { value: s.terminal.font_family }),
    font_size: h("input", { type: "number", min: 8, max: 40, value: s.terminal.font_size }),
    line_height: h("input", { type: "number", min: 1, max: 2, step: 0.05, value: s.terminal.line_height }),
    cursor_style: sel(s.terminal.cursor_style, [["block", "Bloc"], ["bar", "Barre"], ["underline", "Souligné"]]),
    cursor_blink: h("input", { type: "checkbox", checked: s.terminal.cursor_blink }),
    scrollback: h("input", { type: "number", min: 0, max: 200000, step: 1000, value: s.terminal.scrollback }),
    copy_on_select: h("input", { type: "checkbox", checked: s.terminal.copy_on_select }),
    tray_icon: h("input", { type: "checkbox", checked: s.behavior.tray_icon }),
    run_in_background: h("input", { type: "checkbox", checked: s.behavior.run_in_background }),
    check_updates: h("input", { type: "checkbox", checked: s.behavior.check_updates }),
    update_channel: sel(s.behavior.update_channel, [["stable", "Stable"], ["beta", "Bêta"], ["nightly", "Nightly (expérimental)"]]),
    agent_socket: h("input", { value: s.ssh.agent_socket, placeholder: "Auto (ssh config / shell)", spellcheck: false, list: "agents" }),
    keepalive: h("input", { type: "number", min: 0, max: 600, value: s.ssh.keepalive_secs }),
  };
  const agents = h("datalist", { id: "agents" });
  invoke("detect_agents").then((list) => {
    agents.replaceChildren(...list.map(([n, p]) => h("option", { value: p }, n)));
    found.replaceChildren(...(list.length ? list.map(([n, p]) => h("li", {}, h("strong", {}, n), " ", h("code", {}, p))) : [h("li", {}, "Aucun agent détecté")]));
  });
  const found = h("ul.agents", {}, h("li", {}, "Détection…"));

  const kb = { ...s.keybindings };
  const kbRows = Object.entries(ACTIONS).filter(([, [label]]) => label).map(([action, [label]]) => {
    const btn = h("button.btn.small.mono.kbd", { type: "button", "aria-label": `Raccourci pour ${label}` }, prettyAccel(kb[action]));
    btn.onclick = () => {
      btn.textContent = "Appuyez…";
      const grab = (e) => {
        if (["Meta", "Control", "Alt", "Shift"].includes(e.key)) return;
        e.preventDefault(); e.stopPropagation();
        removeEventListener("keydown", grab, true);
        kb[action] = e.key === "Escape" ? kb[action] : e.key === "Backspace" ? "" : accelFromEvent(e);
        btn.textContent = prettyAccel(kb[action]);
      };
      addEventListener("keydown", grab, true);
    };
    return h("div.kb-row", {}, h("span", {}, label), btn,
      h("button.btn.small", { type: "button", onclick: () => { kb[action] = S.defaults[action]; btn.textContent = prettyAccel(kb[action]); } }, "Défaut"));
  });

  const section = (title, ...kids) => h("section.settings-section", {}, h("h3", {}, title), ...kids);
  const check = (input, label) => h("label.check", {}, input, label);
  const ok = await modal("Réglages", (close) => h("form.settings", { onsubmit: (e) => { e.preventDefault(); close(true); } },
    section("Apparence", field("Thème", f.theme), field("Police de l'interface", f.ui_font), field("Taille", f.ui_font_size)),
    section("Terminal", field("Police du terminal", f.font_family), field("Taille", f.font_size), field("Interligne", f.line_height),
      field("Curseur", f.cursor_style), field("Historique (lignes)", f.scrollback), check(f.cursor_blink, "Curseur clignotant"), check(f.copy_on_select, "Copier à la sélection")),
    section("Comportement",
      check(f.tray_icon, isMac() ? "Icône dans la barre des menus" : "Icône dans la zone de notification"),
      check(f.run_in_background, "Rester en arrière-plan à la fermeture de la fenêtre"),
      check(f.check_updates, "Vérifier les mises à jour au lancement et chaque jour"), field("Canal de mise à jour", f.update_channel),
      h("button.btn.small", { type: "button", onclick: () => checkUpdate(true, f.update_channel.value) }, "Vérifier maintenant")),
    section("Agent SSH", field("Socket de l'agent", f.agent_socket, "Vide = détection automatique depuis ~/.ssh/config et votre shell (zsh, bash, fish)."), agents, found,
      field("Keepalive (secondes, 0 = désactivé)", f.keepalive)),
    section("Raccourcis clavier", h("p.mute", {}, "Cliquez puis appuyez sur la combinaison. Retour arrière pour effacer."), ...kbRows),
    h("p.mute.small", {}, `Tout est aussi modifiable dans ${S.configDir}/config.toml (voir docs/CONFIG.md). RustTerm ${S.version}`),
    h("footer", {}, h("button.btn", { type: "button", onclick: () => close(false) }, "Annuler"), h("button.btn.primary", { type: "submit" }, "Enregistrer"))), { wide: true });
  if (!ok) return;

  const num = (el, d) => (Number.isFinite(+el.value) && el.value !== "" ? +el.value : d);
  Object.assign(s.appearance, { theme: f.theme.value, ui_font: f.ui_font.value, ui_font_size: num(f.ui_font_size, 13) });
  Object.assign(s.terminal, { font_family: f.font_family.value, font_size: num(f.font_size, 13), line_height: num(f.line_height, 1.2), cursor_style: f.cursor_style.value, cursor_blink: f.cursor_blink.checked, scrollback: num(f.scrollback, 10000), copy_on_select: f.copy_on_select.checked });
  Object.assign(s.behavior, { tray_icon: f.tray_icon.checked, run_in_background: f.run_in_background.checked, check_updates: f.check_updates.checked, update_channel: f.update_channel.value });
  Object.assign(s.ssh, { agent_socket: f.agent_socket.value.trim(), keepalive_secs: num(f.keepalive, 30) });
  s.keybindings = kb;
  try {
    await invoke("save_settings", { settings: s });
    S.settings = s;
    applyAppearance();
    scheduleUpdates();
  } catch (e) { toast("Réglages : " + e, "error"); }
}

// ---------- updates ----------

let updateTimer;
function scheduleUpdates() {
  clearInterval(updateTimer);
  if (S.settings.behavior.check_updates) updateTimer = setInterval(() => checkUpdate(false), 24 * 3600 * 1000);
}
async function checkUpdate(manual, channel = S.settings.behavior.update_channel) {
  try {
    const u = await invoke("check_update", { channel });
    if (!u) return manual && toast("RustTerm est à jour", "success");
    const [version, notes] = u;
    toast(`Mise à jour ${version} disponible${notes ? " — " + notes.split("\n")[0] : ""}`, "info", [["Installer et redémarrer", async () => {
      toast("Téléchargement de la mise à jour…");
      try { await invoke("install_update", { channel }); } catch (e) { toast("Mise à jour : " + e, "error"); }
    }]]);
  } catch (e) { if (manual) toast("Vérification impossible : " + e, "error"); }
}

// ---------- boot ----------

async function boot() {
  const b = await invoke("boot");
  Object.assign(S, { settings: b.settings, defaults: b.defaults, platform: b.platform, version: b.version, configDir: b.config_dir });
  S.hosts = await invoke("get_hosts");
  document.body.dataset.platform = S.platform;
  document.getElementById("toggle-sidebar").append(icon("sidebar"));
  document.getElementById("toggle-sidebar").onclick = () => setLayout("sidebar");
  document.getElementById("toggle-panel").append(icon("panel"));
  document.getElementById("toggle-panel").onclick = () => setLayout("right_panel");
  document.getElementById("split-h").append(icon("splitH"));
  document.getElementById("split-h").onclick = () => split("right");
  document.getElementById("split-v").append(icon("splitV"));
  document.getElementById("split-v").onclick = () => split("down");
  document.getElementById("open-settings").append(icon("settings"));
  document.getElementById("open-settings").onclick = openSettings;
  initHosts(openHost);
  initPanes();
  initPanel();
  applyAppearance();
  listen("menu", (e) => ACTIONS[e.payload]?.[1]());
  listen("connect-host", (e) => { const host = S.hosts.hosts.find((x) => x.id === e.payload); if (host) newTab(host); });
  if (S.settings.behavior.check_updates) { checkUpdate(false); scheduleUpdates(); }
}

boot().catch((e) => document.body.replaceChildren(h("pre.error", {}, "Erreur au démarrage : " + e)));
