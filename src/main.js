import "@xterm/xterm/css/xterm.css";
import "./style.css";
import { listen } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import { openUrl } from "@tauri-apps/plugin-opener";
import { S, h, icon, invoke, modal, field, toast, isMac } from "./ui.js";
import { THEMES, resolveTheme, applyTheme, xtermTheme } from "./themes.js";
import { t, setLanguage, LANGS, menuLabels } from "./i18n.js";
import { initHosts, render as renderHosts, renderStatus, editHost, editFolder, exportHosts, importHosts } from "./hosts.js";
import { initPanes, openHost, split, closePane, cycleFocus, cycleTab, setZoom, applyTerminalSettings, newTab } from "./panes.js";
import { initPanel, refreshPanel } from "./panel.js";
import { clearLocalHistory } from "./suggest.js";
import appIcon from "../src-tauri/icons/128x128@2x.png";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/ibm-plex-mono/600.css";

const REPO = "https://github.com/midnights-ra1n/RustTerm";

// action -> handler. Labels come from i18n with the same key.
const ACTIONS = {
  new_host: () => editHost(),
  new_folder: () => editFolder(),
  close_pane: closePane,
  split_right: () => split("right"),
  split_left: () => split("left"),
  split_down: () => split("down"),
  split_up: () => split("up"),
  focus_next: () => cycleFocus(1),
  focus_prev: () => cycleFocus(-1),
  next_tab: () => cycleTab(1),
  prev_tab: () => cycleTab(-1),
  search_hosts: () => { setLayout("sidebar", true); document.getElementById("host-search").focus(); },
  toggle_sidebar: () => setLayout("sidebar"),
  toggle_panel: () => setLayout("right_panel"),
  settings: () => openSettings(),
  zoom_in: () => setZoom(1),
  zoom_out: () => setZoom(-1),
  zoom_reset: () => setZoom(0),
  import_hosts: importHosts,
  export_hosts: exportHosts,
  check_update: () => checkUpdate(true),
  about: () => openAbout(),
  open_docs: () => openUrl(`${REPO}/blob/dev/docs/CONFIG.md`),
};
const BINDABLE = ["new_host", "close_pane", "split_right", "split_left", "split_down", "split_up", "focus_next", "focus_prev",
  "next_tab", "prev_tab", "search_hosts", "toggle_sidebar", "toggle_panel", "settings", "zoom_in", "zoom_out", "zoom_reset"];

// ---------- keyboard ----------

const CODE_KEYS = { BracketLeft: "[", BracketRight: "]", Equal: "=", Minus: "-", Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", Backquote: "`" };
function eventKey(e) {
  if (e.key.startsWith("Arrow")) return e.key.slice(5);
  if (/^Key[A-Z]$|^Digit\d$/.test(e.code) && e.key.length !== 1) return e.code.slice(-1);
  return e.key.length === 1 ? e.key.toUpperCase() : e.key;
}
/** Accelerator from an event, e.g. "Cmd+Shift+D" (macOS) / "Ctrl+Alt+D". */
function accelFromEvent(e) {
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
function prettyAccel(a) {
  if (!a) return "—";
  if (!isMac()) return a.replace("CmdOrCtrl", "Ctrl");
  const sym = { cmd: "⌘", cmdorctrl: "⌘", command: "⌘", ctrl: "⌃", control: "⌃", alt: "⌥", option: "⌥", shift: "⇧" };
  const arrows = { Left: "←", Right: "→", Up: "↑", Down: "↓" };
  return a.split("+").map((p) => sym[p.toLowerCase()] || arrows[p] || p).join("");
}

// On macOS the native menu owns the shortcuts; elsewhere we catch them before xterm does.
addEventListener("keydown", (e) => {
  if (isMac() || document.querySelector(".backdrop")) return;
  for (const [action, accel] of Object.entries(S.settings.keybindings)) {
    if (ACTIONS[action] && matches(accel, e)) { e.preventDefault(); e.stopPropagation(); ACTIONS[action](); return; }
  }
}, true);

// ---------- context menu ----------
// Replace the webview menu (Reload / Inspect Element) everywhere; text fields get a native edit menu.
addEventListener("contextmenu", async (e) => {
  if (e.defaultPrevented) return; // a component already showed its own menu
  e.preventDefault();
  const el = e.target.closest?.("input, textarea");
  if (!el || el.type === "checkbox") return;
  const m = await Menu.new({ items: [{ item: "Cut" }, { item: "Copy" }, { item: "Paste" }, { item: "Separator" }, { item: "SelectAll" }] });
  m.popup();
});

// ---------- layout / theme / language ----------

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
  const th = resolveTheme(a.theme, S.settings.themes);
  applyTheme(th);
  const r = document.documentElement.style;
  if (/^#[0-9a-f]{6}$/i.test(a.accent)) r.setProperty("--accent", a.accent);
  document.body.dataset.density = a.density;
  r.setProperty("--ui-font", a.ui_font);
  r.setProperty("--ui-size", a.ui_font_size + "px");
  applyTerminalSettings(xtermTheme(th));
  applyLayout();
}
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => S.settings?.appearance.theme === "system" && applyAppearance());

/** Static chrome labels; dynamic parts re-render themselves. */
function applyLanguage() {
  setLanguage(S.settings.appearance.language);
  const set = (id, key) => { const el = document.getElementById(id); el.setAttribute("aria-label", t(key)); el.title = t(key).replace(/…$/, ""); };
  set("toggle-sidebar", "toggle_sidebar");
  set("add-folder", "new_folder");
  set("add-host", "new_host");
  set("split-h", "split_right");
  set("split-v", "split_down");
  set("toggle-panel", "toggle_panel");
  set("open-settings", "settings");
  const search = document.getElementById("host-search");
  search.placeholder = t("search");
  invoke("set_labels", { labels: menuLabels() }).catch(() => {});
}

// ---------- settings ----------

async function openSettings(section = "general") {
  const s = structuredClone(S.settings);
  const sel = (value, opts) => h("select", {}, ...opts.map(([v, l]) => h("option", { value: v, selected: v === value }, l)));
  const num = (value, min, max, step = 1) => h("input", { type: "number", min, max, step, value });
  const sw = (checked) => h("input.switch", { type: "checkbox", checked, role: "switch" });
  const allThemes = { ...THEMES, ...s.themes };
  const f = {
    language: sel(s.appearance.language, [["auto", t("lang_auto")], ...Object.entries(LANGS)]),
    theme: sel(s.appearance.theme, Object.entries(allThemes).map(([k, th]) => [k, th.label || k])),
    ui_font: h("input", { value: s.appearance.ui_font }),
    ui_font_size: num(s.appearance.ui_font_size, 10, 24),
    accent_on: h("input.switch", { type: "checkbox", checked: !!s.appearance.accent, role: "switch", "aria-label": t("accent_custom") }),
    accent: h("input.color", { type: "color", "aria-label": t("accent_custom"), value: s.appearance.accent || resolveTheme(s.appearance.theme, s.themes).accent || "#C3BAFE" }),
    density: sel(s.appearance.density, [["comfortable", t("density_comfortable")], ["compact", t("density_compact")]]),
    font_weight: sel(s.terminal.font_weight, [["normal", t("fw_normal")], ["500", t("fw_medium")], ["bold", t("fw_bold")]]),
    letter_spacing: num(s.terminal.letter_spacing, -2, 4, 0.5),
    padding: num(s.terminal.padding, 0, 40, 2),
    bold_is_bright: sw(s.terminal.bold_is_bright),
    font_family: h("input", { value: s.terminal.font_family }),
    font_size: num(s.terminal.font_size, 8, 40),
    line_height: num(s.terminal.line_height, 1, 2, 0.05),
    cursor_style: sel(s.terminal.cursor_style, [["block", t("cur_block")], ["bar", t("cur_bar")], ["underline", t("cur_under")]]),
    cursor_blink: sw(s.terminal.cursor_blink),
    scrollback: num(s.terminal.scrollback, 0, 200000, 1000),
    copy_on_select: sw(s.terminal.copy_on_select),
    suggestions: sw(s.terminal.suggestions),
    tmux: sw(s.terminal.tmux),
    tray_icon: sw(s.behavior.tray_icon),
    run_in_background: sw(s.behavior.run_in_background),
    check_updates: sw(s.behavior.check_updates),
    tray_hosts: sel(s.behavior.tray_hosts, [["favorites", t("tray_fav")], ["recent", t("tray_recent")], ["all", t("tray_all")]]),
    tray_count: sel(String(s.behavior.tray_count), [...new Set(["3", "5", "8", "10", "15", "20", String(s.behavior.tray_count)])].map((v) => [v, v])),
    update_channel: sel(s.behavior.update_channel, [["stable", t("ch_stable")], ["beta", t("ch_beta")], ["nightly", t("ch_nightly")]]),
    agent_socket: h("input", { value: s.ssh.agent_socket, placeholder: t("agent_auto"), list: "agents" }),
    keepalive: num(s.ssh.keepalive_secs, 0, 600),
  };
  const agents = h("datalist", { id: "agents" });
  const found = h("ul.agents", {}, h("li.mute", {}, t("detecting")));
  invoke("detect_agents").then((list) => {
    agents.replaceChildren(...list.map(([n, p]) => h("option", { value: p }, n)));
    found.replaceChildren(...(list.length ? list.map(([n, p]) => h("li", {},
      h("button.agent", { type: "button", title: p, onclick: () => { f.agent_socket.value = p; } }, h("strong", {}, n), h("code", {}, p))))
      : [h("li.mute", {}, t("no_agent"))]));
  });

  const kb = { ...s.keybindings };
  const kbRows = BINDABLE.map((action) => {
    const label = t(action).replace(/…$/, "");
    const btn = h("button.kbd", { type: "button", "aria-label": t("shortcut_for", label) }, prettyAccel(kb[action]));
    btn.onclick = () => {
      btn.textContent = t("press");
      btn.classList.add("rec");
      const grab = (e) => {
        if (["Meta", "Control", "Alt", "Shift"].includes(e.key)) return;
        e.preventDefault(); e.stopPropagation();
        removeEventListener("keydown", grab, true);
        btn.classList.remove("rec");
        kb[action] = e.key === "Escape" ? kb[action] : e.key === "Backspace" ? "" : accelFromEvent(e);
        btn.textContent = prettyAccel(kb[action]);
      };
      addEventListener("keydown", grab, true);
    };
    return h("div.kb-row", {}, h("span", {}, label), btn,
      h("button.btn.ghost.small", { type: "button", onclick: () => { kb[action] = S.defaults[action]; btn.textContent = prettyAccel(kb[action]); } }, t("default")));
  });

  const row = (label, input, hint) => h("label.toggle", {}, h("span", {}, label, hint && h("small", {}, hint)), input);
  const pages = {
    general: [t("set_general"), h("div.form-grid", {}, field(t("language"), f.language), field(t("channel"), f.update_channel)),
      h("div.form-grid", {}, field(t("tray_hosts"), f.tray_hosts), field(t("tray_count"), f.tray_count)),
      h("div.toggles", {},
        row(isMac() ? t("tray_mac") : t("tray_other"), f.tray_icon),
        row(t("background"), f.run_in_background),
        row(t("auto_updates"), f.check_updates)),
      h("div.actions", {}, h("button.btn.small", { type: "button", onclick: () => checkUpdate(true, f.update_channel.value) }, t("check_now")))],
    appearance: [t("set_appearance"), h("div.form-grid", {}, field(t("theme"), f.theme), field(t("density"), f.density),
      h("div.span2", {}, field(t("ui_font"), f.ui_font)), field(t("size"), f.ui_font_size)),
      h("div.toggles", {}, h("div.toggle", {}, h("span", {}, t("accent_custom"), h("small", {}, t("accent_hint"))), h("div.inline", {}, f.accent, f.accent_on)))],
    terminal: [t("set_terminal"), h("div.form-grid", {},
      h("div.span2", {}, field(t("term_font"), f.font_family)),
      field(t("size"), f.font_size), field(t("line_height"), f.line_height),
      field(t("cursor"), f.cursor_style), field(t("scrollback"), f.scrollback),
      field(t("font_weight"), f.font_weight), field(t("letter_spacing"), f.letter_spacing), field(t("term_padding"), f.padding)),
      h("div.toggles", {}, row(t("blink"), f.cursor_blink), row(t("bold_bright"), f.bold_is_bright), row(t("copy_select"), f.copy_on_select), row(t("suggestions"), f.suggestions, t("sugg_hint")), row(t("tmux_setting"), f.tmux, t("tmux_setting_hint"))),
      h("div.actions", {}, h("button.btn.small", { type: "button", onclick: () => { clearLocalHistory(); toast(t("history_cleared"), "success"); } }, t("clear_history")))],
    ssh: [t("set_ssh"), h("div.form-grid", {}, h("div.span2", {}, field(t("agent_socket"), f.agent_socket, t("agent_hint"))), agents,
      field(t("keepalive"), f.keepalive)),
      h("h4", {}, t("agents_found")), found],
    keys: [t("set_keys"), h("p.mute.small", {}, t("keys_hint")), h("div.kb-list", {}, ...kbRows)],
    about: [t("set_about"), aboutView()],
  };

  const content = h("div.settings-content");
  const nav = h("nav.settings-nav", { role: "tablist", "aria-orientation": "vertical" });
  const show = (key) => {
    section = key;
    const [title, ...kids] = pages[key];
    content.replaceChildren(h("h3", {}, title), ...kids);
    nav.querySelectorAll("button").forEach((b) => { b.classList.toggle("on", b.dataset.k === key); b.setAttribute("aria-selected", String(b.dataset.k === key)); });
  };
  nav.append(...Object.entries(pages).map(([k, [title]]) => h("button", { type: "button", role: "tab", dataset: { k }, onclick: () => show(k) }, title)));
  show(section);

  const ok = await modal(t("settings_title"), (close) => h("form.settings", { onsubmit: (e) => { e.preventDefault(); close(true); } },
    h("div.settings-body", {}, nav, content),
    h("footer", {}, h("span.mute.small.grow", {}, t("config_file", `${S.configDir}/config.toml`)),
      h("button.btn", { type: "button", onclick: () => close(false) }, t("cancel")), h("button.btn.primary", { type: "submit" }, t("save")))),
  { wide: true, cls: "settings-modal" });
  if (!ok) return;

  const n = (el, d) => (el.value !== "" && Number.isFinite(+el.value) ? +el.value : d);
  Object.assign(s.appearance, { language: f.language.value, theme: f.theme.value, ui_font: f.ui_font.value, ui_font_size: n(f.ui_font_size, 13), accent: f.accent_on.checked ? f.accent.value : "", density: f.density.value });
  Object.assign(s.terminal, { font_family: f.font_family.value, font_size: n(f.font_size, 13), line_height: n(f.line_height, 1.2), cursor_style: f.cursor_style.value, cursor_blink: f.cursor_blink.checked, scrollback: n(f.scrollback, 10000), copy_on_select: f.copy_on_select.checked, suggestions: f.suggestions.checked, tmux: f.tmux.checked, font_weight: f.font_weight.value, letter_spacing: n(f.letter_spacing, 0), padding: n(f.padding, 8), bold_is_bright: f.bold_is_bright.checked });
  Object.assign(s.behavior, { tray_icon: f.tray_icon.checked, run_in_background: f.run_in_background.checked, check_updates: f.check_updates.checked, update_channel: f.update_channel.value, tray_hosts: f.tray_hosts.value, tray_count: +f.tray_count.value || 5 });
  Object.assign(s.ssh, { agent_socket: f.agent_socket.value.trim(), keepalive_secs: n(f.keepalive, 30) });
  s.keybindings = kb;
  const langChanged = s.appearance.language !== S.settings.appearance.language;
  try {
    await invoke("save_settings", { settings: s });
    S.settings = s;
    applyAppearance();
    if (langChanged) { applyLanguage(); initPanel(); renderHosts(); initPanes(); }
    scheduleUpdates();
  } catch (e) { toast(`${t("settings_title")} : ${e}`, "error"); }
}

// ---------- about ----------

function openAbout() {
  return modal(t("about"), () => aboutView(), { cls: "about-modal" });
}

/** Shared by the About window (menu) and the About page of the settings. */
function aboutView() {
  const plat = { macos: "macOS", windows: "Windows", linux: "Linux" }[S.platform] || S.platform;
  const ch = S.settings.behavior.update_channel;
  return h("div.about", {},
    h("img.about-icon", { src: appIcon, alt: "", width: 112, height: 112 }),
    h("h1.about-name", {}, "Termix"),
    h("p.about-tag", {}, t("about_tag")),
    h("dl.about-meta", {},
      h("dt", {}, t("version")), h("dd.mono", {}, `${S.version} · ${t("ch_" + ch)}`),
      h("dt", {}, t("platform")), h("dd.mono", {}, `${plat} · ${S.arch}`),
      h("dt", {}, t("config_dir")), h("dd.mono.wrap", {}, S.configDir)),
    h("div.about-actions", {},
      h("button.btn.primary", { type: "button", onclick: () => checkUpdate(true) }, t("check_update").replace(/…$/, "")),
      h("button.btn", { type: "button", onclick: () => openUrl(REPO) }, t("github")),
      h("button.btn", { type: "button", onclick: () => openUrl(`${REPO}/releases`) }, t("release_notes"))),
    h("p.about-foot.mute.small", {}, t("built_with")));
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
    if (!u) return manual && toast(t("up_to_date"), "success");
    const [version, notes] = u;
    toast(`${t("update_avail", version)}${notes ? " — " + notes.split("\n")[0] : ""}`, "info", [[t("install_restart"), async () => {
      toast(t("downloading"));
      try { await invoke("install_update", { channel }); } catch (e) { toast(t("update_err", e), "error"); }
    }]]);
  } catch (e) { if (manual) toast(t("check_err", e), "error"); }
}

// ---------- boot ----------

async function boot() {
  const [b, hosts, status] = await Promise.all([invoke("boot"), invoke("get_hosts"), invoke("host_status")]);
  Object.assign(S, { settings: b.settings, defaults: b.defaults, platform: b.platform, arch: b.arch, version: b.version, configDir: b.config_dir, hosts, status });
  document.body.dataset.platform = S.platform;
  for (const [id, name] of [["toggle-sidebar", "sidebar"], ["add-folder", "folderPlus"], ["add-host", "plus"], ["split-h", "splitH"], ["split-v", "splitV"], ["toggle-panel", "panel"], ["open-settings", "settings"]]) {
    document.getElementById(id).append(icon(name, 16));
  }
  document.getElementById("toggle-sidebar").onclick = () => setLayout("sidebar");
  document.getElementById("add-folder").onclick = () => editFolder();
  document.getElementById("add-host").onclick = () => editHost();
  document.getElementById("toggle-panel").onclick = () => setLayout("right_panel");
  document.getElementById("split-h").onclick = () => split("right");
  document.getElementById("split-v").onclick = () => split("down");
  document.getElementById("open-settings").onclick = () => openSettings();
  applyLanguage();
  initHosts(openHost);
  initPanes();
  initPanel();
  applyAppearance();
  // Show the window only once the first frame is ready: no white flash.
  requestAnimationFrame(() => invoke("app_ready"));
  listen("menu", (e) => ACTIONS[e.payload]?.());
  listen("connect-host", (e) => { const host = S.hosts.hosts.find((x) => x.id === e.payload); if (host) newTab(host); });
  listen("host-status", (e) => { S.status = e.payload; renderStatus(); });
  if (S.settings.behavior.check_updates) { setTimeout(() => checkUpdate(false), 2000); scheduleUpdates(); }
}

boot().catch((e) => {
  document.body.replaceChildren(h("pre.error", {}, t("boot_error", e)));
  invoke("app_ready");
});
