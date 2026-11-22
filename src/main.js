import "@xterm/xterm/css/xterm.css";
import "./style.css";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { S, h, icon, invoke, modal, field, toast, isMac, isIOS, contextMenu, openDialog } from "./ui.js";
import { THEMES, resolveTheme, applyTheme, xtermTheme } from "./themes.js";
import { t, setLanguage, LANGS, menuLabels } from "./i18n.js";
import { initHosts, render as renderHosts, renderStatus, editHost, editFolder, exportHosts, importHosts } from "./hosts.js";
import { initPanes, openHost, split, closePane, cycleFocus, cycleTab, setZoom, applyTerminalSettings, newTab, focusTerminal, quickConnect, clearScreen,
  tabList, selectTabAt, closeTabAt, keyInput, ctrlOn } from "./panes.js";
import { initMenubar, refreshMenubar } from "./menubar.js";
import { initPanel, refreshPanel, PANEL_TABS, panelTab, setPanelTab, panelActions } from "./panel.js";
import { initNative, refreshChrome, SF } from "./native.js";
import { clearLocalHistory } from "./suggest.js";
import { syncView, initSyncStatus } from "./sync.js";
import appIcon from "../src-tauri/icons/128x128@2x.png";
import { UI_FONTS, TERM_FONTS } from "./fonts.js";

const REPO = "https://github.com/midnights-ra1n/termix";

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
  new_tab: () => newTab(),
  quick_connect: () => quickConnect(),
  clear_screen: clearScreen,
  refresh_status: () => invoke("refresh_status"),
  shortcuts: () => openSettings("keys"),
  release_notes: () => openUrl(`${REPO}/releases`),
  report_issue: () => openUrl(`${REPO}/issues/new`),
};
const BINDABLE = ["new_tab", "quick_connect", "new_host", "close_pane", "split_right", "split_left", "split_down", "split_up", "focus_next", "focus_prev",
  "next_tab", "prev_tab", "search_hosts", "toggle_sidebar", "toggle_panel", "settings", "zoom_in", "zoom_out", "zoom_reset", "clear_screen"];

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

// On macOS the native menu owns the shortcuts; elsewhere (iPad keyboards too) we catch them before xterm does.
addEventListener("keydown", (e) => {
  if (S.platform === "macos" || document.querySelector(".backdrop")) return;
  for (const [action, accel] of Object.entries(S.settings.keybindings)) {
    if (ACTIONS[action] && matches(accel, e)) { e.preventDefault(); e.stopPropagation(); ACTIONS[action](); return; }
  }
}, true);

// ---------- context menu ----------
// Replace the webview menu (Reload / Inspect Element) everywhere; text fields get an edit menu.
addEventListener("contextmenu", (e) => {
  if (e.defaultPrevented) return; // a component already showed its own menu
  e.preventDefault();
  const el = e.target.closest?.("input, textarea");
  if (!el || el.type === "checkbox") return;
  el.focus();
  const sel = el.selectionStart !== el.selectionEnd, ro = el.readOnly || el.disabled;
  contextMenu(e, [
    { label: t("cut"), icon: "cut", disabled: !sel || ro, action: () => document.execCommand("cut") },
    { label: t("copy"), icon: "copy", disabled: !sel, action: () => document.execCommand("copy") },
    { label: t("paste"), icon: "paste", disabled: ro, action: async () => document.execCommand("insertText", false, await navigator.clipboard.readText()) },
    "-",
    { label: t("select_all"), icon: "selectAll", action: () => el.select() },
  ]);
});

// ---------- touch (iOS) ----------

// No right click: a long press opens the same menus (hosts, folders, terminal, text fields).
let press = null;
addEventListener("touchstart", (e) => {
  clearTimeout(press?.timer);
  if (!isIOS() || e.touches.length !== 1) return (press = null);
  const { clientX, clientY } = e.touches[0], target = e.target;
  press = { x: clientX, y: clientY, fired: false, timer: setTimeout(() => {
    press.fired = true;
    target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX, clientY }));
  }, 500) };
}, { passive: true });
addEventListener("touchmove", (e) => {
  const p = e.touches[0];
  if (press && Math.hypot(p.clientX - press.x, p.clientY - press.y) > 10) clearTimeout(press.timer);
}, { passive: true });
addEventListener("touchend", (e) => {
  clearTimeout(press?.timer);
  if (press?.fired) e.preventDefault(); // no click after the menu: it would close it right away
}, { passive: false });
// One tap opens what a double click opens on a computer (host, file, folder of the SFTP panel).
addEventListener("click", (e) => isIOS() && e.target.closest?.(".row:not(.folder)")?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
addEventListener("dblclick", (e) => isIOS() && e.isTrusted && e.stopImmediatePropagation(), true); // a double tap would open twice

// The on-screen keyboard only shrinks the visual viewport: size the page to it so the terminal stays above.
visualViewport?.addEventListener("resize", () => {
  if (!isIOS()) return;
  const kb = innerHeight - visualViewport.height > 80;
  document.documentElement.style.height = visualViewport.height + "px";
  document.body.classList.toggle("kb-open", kb);
  // The terminal follows the keyboard; a form field (bottom sheet) needs iOS to pan up to it.
  if (!kb || document.activeElement?.closest(".xterm")) scrollTo(0, 0);
});

// ---------- layout / theme / language ----------

// iPhone: the side bars are drawers over the terminal, one at a time. Not saved: the settings are synced
// with the computers, whose side bars must not open or close with the phone's.
const phoneMq = matchMedia("(max-width: 700px)");
const isPhone = () => isIOS() && phoneMq.matches;
let drawer = null;
const shown = (key) => (isPhone() ? drawer === key : S.settings.appearance[key]);
phoneMq.addEventListener("change", () => S.settings && applyLayout());

function setLayout(key, value) {
  if (isPhone()) {
    const on = value ?? drawer !== key;
    drawer = on ? key : drawer === key ? null : drawer;
    return applyLayout();
  }
  const a = S.settings.appearance;
  a[key] = value ?? !a[key];
  applyLayout();
  invoke("save_settings", { settings: S.settings }).catch(() => {});
}
function applyLayout() {
  document.body.classList.toggle("phone", isPhone());
  document.body.classList.toggle("no-sidebar", !shown("sidebar"));
  document.body.classList.toggle("no-panel", !shown("right_panel"));
  applyWidths();
  refreshPanel();
  refreshChrome();
}
function applyWidths() {
  const a = S.settings.appearance;
  // On :root so body.no-sidebar / .no-panel (0px) still win.
  document.documentElement.style.setProperty("--sw", a.sidebar_width + "px");
  document.documentElement.style.setProperty("--pw", a.panel_width + "px");
}

/** Drag the inner edge of the sidebar / side panel to resize it; double-click restores the default width. */
const WIDTHS = { sidebar_width: 248, panel_width: 300 };
function resizeEdge(aside, key, left) {
  const clamp = (w) => Math.round(Math.max(180, Math.min(w, 640, innerWidth * 0.45)));
  aside.append(h("div.edge", { role: "separator", "aria-orientation": "vertical",
    ondblclick: () => { S.settings.appearance[key] = WIDTHS[key]; applyWidths(); invoke("save_settings", { settings: S.settings }).catch(() => {}); },
    onmousedown: (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const r = aside.getBoundingClientRect();
      document.body.classList.add("resizing"); // no grid transition while dragging
      const move = (ev) => { S.settings.appearance[key] = clamp(left ? ev.clientX - r.left : r.right - ev.clientX); applyWidths(); };
      const up = () => {
        removeEventListener("mousemove", move); removeEventListener("mouseup", up);
        document.body.classList.remove("resizing");
        invoke("save_settings", { settings: S.settings }).catch(() => {});
      };
      addEventListener("mousemove", move); addEventListener("mouseup", up);
    } }));
}
resizeEdge(document.getElementById("sidebar"), "sidebar_width", true);
resizeEdge(document.getElementById("panel"), "panel_width", false);

// Colours the user can override on top of any theme (Appearance → Colours, or [appearance.colors]).
const COLOR_KEYS = ["accent", "bg", "scene", "ink", "term_bg", "term_fg", "cursor", "selection"];
const userColors = (a) => Object.fromEntries(Object.entries({ accent: a.accent, ...a.colors }).filter(([, v]) => /^#[0-9a-f]{6}$/i.test(v)));

/** Bundled fonts + "custom", which shows a field for any installed font stack. `.read()` gives the CSS stack. */
function fontPicker(value, list) {
  const known = list.some(([, v]) => v === value);
  const sel = h("select", {}, ...list.map(([l, v]) => h("option", { value: v, selected: v === value, style: `font-family: ${v}` }, l === "System" ? t("font_system") : l)),
    h("option", { value: "", selected: !known }, t("font_custom")));
  const custom = h("input", { value: known ? "" : value, placeholder: '"My Font", monospace', hidden: known, "aria-label": t("font_custom") });
  sel.onchange = () => { custom.hidden = sel.value !== ""; if (!custom.hidden) custom.focus(); };
  const el = h("div.font-picker", {}, sel, custom);
  el.read = () => sel.value || custom.value.trim() || list[0][1];
  return el;
}

function applyAppearance() {
  const a = S.settings.appearance;
  const th = { ...resolveTheme(a.theme, S.settings.themes), ...userColors(a) };
  applyTheme(th);
  const r = document.documentElement.style;
  document.body.dataset.density = a.density;
  r.setProperty("--ui-font", a.ui_font);
  r.setProperty("--ui-size", a.ui_font_size + "px");
  applyTerminalSettings(xtermTheme(th));
  applyMaterial();
  applyLayout();
}

/** Window materials each system offers (Appearance → Window); Linux can only blur, on KDE Plasma.
 *  iOS draws its glass itself (style.css): nothing is behind a full-screen app. */
const MATERIALS = { macos: ["vibrancy", "liquid_glass"], windows: ["acrylic", "mica"], linux: ["blur"], ios: ["liquid_glass"] };
const isKde = () => /KDE/i.test(document.documentElement.dataset.desktop || "");
const materialOn = (s = S.settings) => (MATERIALS[S.platform] || []).includes(s.window.material) && (S.platform !== "linux" || isKde());
/** macOS / Windows windows are transparent only if created so (Linux always is): a change needs a restart. */
const needsTransparentWindow = (s) => S.platform === "linux" || (!isIOS() && (s.terminal.background_opacity < 100 || materialOn(s)));
let transparentAtLaunch = true;

// Linux: KDE blurs behind the window along #app's rounded outline (inside the shadow margin), kept up to date.
let blurObs = null;
const blurSend = () => {
  const app = document.getElementById("app"), r = app.getBoundingClientRect();
  invoke("window_blur", { rect: [r.x, r.y, r.width, r.height, parseFloat(getComputedStyle(app).borderTopLeftRadius) || 0] }).catch(() => {});
};
// Hidden then shown again (run in background): GTK gives the window a new surface, without the blur.
addEventListener("focus", () => blurObs && blurSend());
document.addEventListener("visibilitychange", () => blurObs && !document.hidden && blurSend());

function applyMaterial() {
  const on = materialOn();
  document.body.classList.toggle("material", on);
  document.documentElement.style.setProperty("--ui-op", S.settings.window.interface_opacity + "%");
  if (S.platform !== "linux" || on === !!blurObs) return;
  if (on) {
    blurObs = new ResizeObserver(() => requestAnimationFrame(blurSend)); // also fires right away
    blurObs.observe(document.getElementById("app"));
  } else {
    blurObs.disconnect();
    blurObs = null;
    invoke("window_blur", { rect: null }).catch(() => {});
  }
}
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => S.settings?.appearance.theme === "system" && applyAppearance());

/** Static chrome labels; dynamic parts re-render themselves. */
function applyLanguage() {
  setLanguage(S.settings.appearance.language);
  const set = (id, key) => { const el = document.getElementById(id); el.setAttribute("aria-label", t(key)); el.title = t(key).replace(/…$/, ""); };
  set("toggle-sidebar", "toggle_sidebar");
  set("add-folder", "new_folder");
  set("add-host", "new_host");
  set("clear-term", "clear_screen");
  set("split-h", "split_right");
  set("split-v", "split_down");
  set("toggle-panel", "toggle_panel");
  set("open-settings", "settings");
  const search = document.getElementById("host-search");
  search.placeholder = t("search");
  invoke("set_labels", { labels: menuLabels() }).then(refreshMenubar).catch(() => {});
  refreshChrome();
}

// ---------- iOS: SwiftUI title bar and key bar (native.js) ----------

/** What they show; sent again by refreshChrome() whenever it changes. */
function chromeState() {
  const sync = document.getElementById("sync-status");
  const item = (id, ic) => ({ id, l: t(id).replace(/…$/, ""), sf: SF[ic] });
  return {
    tabs: tabList(), sidebar: shown("sidebar"), panel: shown("right_panel"), compact: isPhone(),
    typing: document.body.classList.contains("typing"), ctrl: ctrlOn(),
    panelTab: panelTab(), panelTabs: PANEL_TABS.map(([k, ic]) => ({ id: k, l: t(k), sf: SF[ic] })),
    panelMenu: panelActions().map((a, i) => ({ id: "panel:" + i, l: a.label, sf: SF[a.icon] })),
    plus: [item("quick_connect", "terminal"), item("new_host", "plus"), item("new_folder", "folderPlus"), { sep: true },
      item("split_right", "splitH"), item("split_down", "splitV"), item("clear_screen", "clear"), { sep: true },
      item("import_hosts", "download"), item("export_hosts", "upload")],
    sync: sync.hidden ? null : SF[sync.dataset.icon] || null, syncAlert: /warn|error/.test(sync.className),
    tint: getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#8C7CF0",
    dark: document.documentElement.dataset.dark !== "0",
    l: { sidebar: t("toggle_sidebar"), panel: t("toggle_panel"), settings: t("settings").replace(/…$/, ""), new_tab: t("new_tab"),
      close_tab: t("close_tab"), sync: sync.title, panel_actions: t("panel_actions"), kb_hide: t("kb_hide") },
  };
}

function nativeAction(id) {
  const [kind, arg] = id.split(":");
  if (kind === "tab") { selectTabAt(+arg); if (isPhone() && drawer) setLayout(drawer, false); }
  else if (kind === "close_tab") closeTabAt(+arg);
  else if (kind === "panel_tab") setPanelTab(arg);
  else if (kind === "panel") panelActions()[+arg]?.action();
  else if (id === "sync") document.getElementById("sync-status").click();
  else ACTIONS[id]?.();
}

// ---------- settings ----------

/** Linux windows are always transparent (CSS corners); iOS draws everything in the page: no restart needed. */
const isLinuxLike = () => S.platform === "linux" || isIOS();

async function openSettings(section = "general") {
  const s = structuredClone(S.settings);
  const sel = (value, opts) => h("select", {}, ...opts.map(([v, l]) => h("option", { value: v, selected: v === value }, l)));
  const num = (value, min, max, step = 1) => h("input", { type: "number", min, max, step, value });
  const sw = (checked) => h("input.switch", { type: "checkbox", checked, role: "switch" });
  /** Slider with its value shown next to it; `.input` is the <input> (value read on save). */
  const range = (value, min, max, unit = "%") => {
    const input = h("input", { type: "range", min, max, value });
    const out = h("output", {}, value + unit);
    input.oninput = () => { out.value = input.value + unit; };
    const el = h("div.range", {}, input, out);
    el.input = input;
    return el;
  };
  const bgImage = h("input", { value: s.terminal.background_image, placeholder: t("bg_none") });
  const imgPick = h("div.file-pick", {}, bgImage,
    h("button.btn.small", { type: "button", onclick: async () => {
      const p = await openDialog({ filters: [{ name: t("bg_images"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "avif", "bmp"] }] });
      if (typeof p === "string") bgImage.value = p;
    } }, t("bg_choose")),
    h("button.icon-btn", { type: "button", "aria-label": t("bg_remove"), title: t("bg_remove"), onclick: () => { bgImage.value = ""; } }, icon("close", 14)));
  const allThemes = { ...THEMES, ...s.themes };
  const f = {
    language: sel(s.appearance.language, [["auto", t("lang_auto")], ...Object.entries(LANGS)]),
    theme: sel(s.appearance.theme, Object.entries(allThemes).map(([k, th]) => [k, th.label || k])),
    ui_font: fontPicker(s.appearance.ui_font, UI_FONTS),
    ui_font_size: num(s.appearance.ui_font_size, 10, 24),
    density: sel(s.appearance.density, [["comfortable", t("density_comfortable")], ["compact", t("density_compact")]]),
    font_weight: sel(s.terminal.font_weight, [["normal", t("fw_normal")], ["500", t("fw_medium")], ["bold", t("fw_bold")]]),
    letter_spacing: num(s.terminal.letter_spacing, -2, 4, 0.5),
    padding: num(s.terminal.padding, 0, 40, 2),
    bold_is_bright: sw(s.terminal.bold_is_bright),
    focus_single: sw(s.terminal.focus_single),
    focus_width: num(s.terminal.focus_width, 1, 6),
    font_family: fontPicker(s.terminal.font_family, TERM_FONTS),
    font_size: num(s.terminal.font_size, 8, 40),
    line_height: num(s.terminal.line_height, 1, 2, 0.05),
    cursor_style: sel(s.terminal.cursor_style, [["block", t("cur_block")], ["bar", t("cur_bar")], ["underline", t("cur_under")]]),
    cursor_blink: sw(s.terminal.cursor_blink),
    scrollback: num(s.terminal.scrollback, 0, 200000, 1000),
    copy_on_select: sw(s.terminal.copy_on_select),
    suggestions: sw(s.terminal.suggestions),
    tmux: sw(s.terminal.tmux),
    keep_focus: sw(s.terminal.keep_focus),
    tray_icon: sw(s.behavior.tray_icon),
    run_in_background: sw(s.behavior.run_in_background),
    multi_instance: sw(s.behavior.multi_instance),
    check_updates: sw(s.behavior.check_updates),
    tray_hosts: sel(s.behavior.tray_hosts, [["favorites", t("tray_fav")], ["recent", t("tray_recent")], ["all", t("tray_all")]]),
    tray_count: sel(String(s.behavior.tray_count), [...new Set(["3", "5", "8", "10", "15", "20", String(s.behavior.tray_count)])].map((v) => [v, v])),
    update_channel: sel(s.behavior.update_channel, [["stable", t("ch_stable")], ["beta", t("ch_beta")], ["nightly", t("ch_nightly")]]),
    agent_socket: h("input", { value: s.ssh.agent_socket, placeholder: t("agent_auto"), list: "agents" }),
    keepalive: num(s.ssh.keepalive_secs, 0, 600),
    bg_opacity: range(s.terminal.background_opacity, 0, 100),
    bg_blur: range(s.terminal.background_blur, 0, 40, " px"),
    bg_image_opacity: range(s.terminal.background_image_opacity, 0, 100),
    bg_fit: sel(s.terminal.background_image_fit, ["cover", "contain", "tile", "center", "stretch"].map((v) => [v, t("fit_" + v)])),
    bg_x: sel(s.terminal.background_image_x, ["left", "center", "right"].map((v) => [v, t("x_" + v)])),
    bg_y: sel(s.terminal.background_image_y, ["top", "center", "bottom"].map((v) => [v, t("y_" + v)])),
    gpu: sw(s.performance.gpu),
    animations: sw(s.performance.animations),
    probe_secs: sel(String(s.performance.probe_secs), [...new Set(["10", "30", "60", "300", "0", String(s.performance.probe_secs)])].map((v) => [v, v === "0" ? t("probe_manual") : t("every_secs", v)])),
    material: sel(s.window.material, [["none", t("mat_none")], ...(MATERIALS[S.platform] || []).map((m) => [m, t("mat_" + m)])]),
    ui_opacity: range(s.window.interface_opacity, 20, 100),
    stats_secs: sel(String(s.performance.stats_secs), [...new Set(["1", "2", "3", "5", "10", String(s.performance.stats_secs)])].map((v) => [v, t("every_secs", v)])),
  };
  f.material.disabled = S.platform === "linux" && !isKde(); // GNOME & co. have no blur-behind API
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

  const custom = userColors(s.appearance), base = resolveTheme(s.appearance.theme, s.themes);
  const colors = COLOR_KEYS.map((k) => {
    const label = t("c_" + k);
    const on = h("input.switch", { type: "checkbox", checked: k in custom, role: "switch", "aria-label": label });
    const pick = h("input.color", { type: "color", "aria-label": label, value: custom[k] || base[k] || (k === "term_bg" ? base.bg : k === "term_fg" ? base.ink : "#888888") });
    pick.oninput = () => { on.checked = true; };
    return { k, on, pick, el: h("div.toggle", {}, h("span", {}, label), h("div.inline", {}, pick, on)) };
  });
  const row = (label, input, hint) => h("label.toggle", {}, h("span", {}, label, hint && h("small", {}, hint)), input);
  // iOS: no menu bar icon, background mode, instances or self-update (App Store / TestFlight), no SSH agents.
  const desk = !isIOS();
  const pages = {
    general: [t("set_general"), h("div.form-grid", {}, field(t("language"), f.language), desk && field(t("channel"), f.update_channel)),
      desk && h("div.form-grid", {}, field(t("tray_hosts"), f.tray_hosts), field(t("tray_count"), f.tray_count)),
      desk && h("div.toggles", {},
        row(isMac() ? t("tray_mac") : t("tray_other"), f.tray_icon),
        row(t("background"), f.run_in_background),
        row(t("multi_instance"), f.multi_instance, t("multi_instance_hint")),
        row(t("auto_updates"), f.check_updates)),
      desk && h("div.actions", {}, h("button.btn.small", { type: "button", onclick: () => checkUpdate(true, f.update_channel.value) }, t("check_now")))],
    appearance: [t("set_appearance"), h("div.form-grid", {}, field(t("theme"), f.theme), field(t("density"), f.density),
      h("div.span2", {}, field(t("ui_font"), f.ui_font)), field(t("size"), f.ui_font_size)),
      h("h4", {}, t("colors"), h("small.mute", {}, " — ", t("colors_hint"))), h("div.toggles", {}, ...colors.map((c) => c.el)),
      h("h4", {}, desk ? t("win_title") : t("mat_liquid_glass")),
      h("div.form-grid", {},
        field(t(desk ? "material" : "material_ios"), f.material, t("material_hint_" + (S.platform === "linux" && !isKde() ? "nokde" : S.platform))),
        field(t("ui_opacity"), f.ui_opacity, t("ui_opacity_hint")))],
    terminal: [t("set_terminal"), h("div.form-grid", {},
      h("div.span2", {}, field(t("term_font"), f.font_family)),
      field(t("size"), f.font_size), field(t("line_height"), f.line_height),
      field(t("cursor"), f.cursor_style), field(t("scrollback"), f.scrollback),
      field(t("font_weight"), f.font_weight), field(t("letter_spacing"), f.letter_spacing), field(t("term_padding"), f.padding), field(t("focus_width"), f.focus_width)),
      h("div.toggles", {}, row(t("blink"), f.cursor_blink), row(t("bold_bright"), f.bold_is_bright), row(t("copy_select"), f.copy_on_select), row(t("suggestions"), f.suggestions, t("sugg_hint")), row(t("tmux_setting"), f.tmux, t("tmux_setting_hint")), row(t("keep_focus"), f.keep_focus, t("keep_focus_hint")), row(t("focus_single"), f.focus_single, t("focus_single_hint"))),
      h("div.actions", {}, h("button.btn.small", { type: "button", onclick: () => { clearLocalHistory(); toast(t("history_cleared"), "success"); } }, t("clear_history"))),
      h("h4", {}, t("bg_title")),
      h("div.form-grid", {},
        field(t("bg_opacity"), f.bg_opacity, t(isLinuxLike() ? "bg_opacity_hint_linux" : "bg_opacity_hint")),
        field(t("bg_blur"), f.bg_blur, t("bg_blur_hint")),
        h("div.span2", {}, field(t("bg_image"), imgPick)),
        field(t("bg_image_opacity"), f.bg_image_opacity), field(t("bg_fit"), f.bg_fit),
        field(t("bg_x"), f.bg_x), field(t("bg_y"), f.bg_y))],
    performance: [t("set_perf"), h("div.form-grid", {},
      field(t("perf_probe"), f.probe_secs, t("perf_probe_hint")), field(t("perf_stats"), f.stats_secs)),
      h("div.toggles", {}, row(t("perf_gpu"), f.gpu, t("perf_gpu_hint")), row(t("perf_anim"), f.animations)),
      h("p.mute.small", {}, t("perf_scrollback_hint"))],
    ssh: [t("set_ssh"), h("div.form-grid", {}, desk && h("div.span2", {}, field(t("agent_socket"), f.agent_socket, t("agent_hint"))), agents,
      field(t("keepalive"), f.keepalive)),
      desk && h("h4", {}, t("agents_found")), desk && found],
    keys: [t("set_keys"), h("p.mute.small", {}, t("keys_hint")), h("div.kb-list", {}, ...kbRows)],
    sync: [t("set_sync"), syncView()],
    about: [t("set_about"), aboutView()],
  };
  if (isIOS()) delete pages.keys; // recording a shortcut needs the page's key events, which a native sheet keeps

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
  Object.assign(s.appearance, { language: f.language.value, theme: f.theme.value, ui_font: f.ui_font.read(), ui_font_size: n(f.ui_font_size, 13), accent: "", colors: Object.fromEntries(colors.filter((c) => c.on.checked).map((c) => [c.k, c.pick.value])), density: f.density.value });
  Object.assign(s.terminal, { font_family: f.font_family.read(), font_size: n(f.font_size, 13), line_height: n(f.line_height, 1.2), cursor_style: f.cursor_style.value, cursor_blink: f.cursor_blink.checked, scrollback: n(f.scrollback, 10000), copy_on_select: f.copy_on_select.checked, suggestions: f.suggestions.checked, tmux: f.tmux.checked, keep_focus: f.keep_focus.checked, font_weight: f.font_weight.value, letter_spacing: n(f.letter_spacing, 0), padding: n(f.padding, 8), bold_is_bright: f.bold_is_bright.checked, focus_single: f.focus_single.checked, focus_width: Math.min(6, Math.max(1, n(f.focus_width, 2))) });
  Object.assign(s.behavior, { tray_icon: f.tray_icon.checked, run_in_background: f.run_in_background.checked, multi_instance: f.multi_instance.checked, check_updates: f.check_updates.checked, update_channel: f.update_channel.value, tray_hosts: f.tray_hosts.value, tray_count: +f.tray_count.value || 5 });
  Object.assign(s.ssh, { agent_socket: f.agent_socket.value.trim(), keepalive_secs: n(f.keepalive, 30) });
  // A picked file is copied into the config folder (shrunk if big), so the sync can carry it to other devices.
  let img = bgImage.value.trim();
  if (img !== s.terminal.background_image && /[\\/]/.test(img)) {
    try { img = await invoke("import_background", { path: img }); } catch (e) { toast(String(e), "error"); img = s.terminal.background_image; }
  }
  Object.assign(s.window, { material: f.material.value, interface_opacity: +f.ui_opacity.input.value });
  Object.assign(s.terminal, { background_opacity: +f.bg_opacity.input.value, background_blur: +f.bg_blur.input.value, background_image: img,
    background_image_opacity: +f.bg_image_opacity.input.value, background_image_fit: f.bg_fit.value, background_image_x: f.bg_x.value, background_image_y: f.bg_y.value });
  Object.assign(s.performance, { gpu: f.gpu.checked, animations: f.animations.checked, probe_secs: +f.probe_secs.value, stats_secs: +f.stats_secs.value || 3 });
  s.keybindings = kb;
  const langChanged = s.appearance.language !== S.settings.appearance.language;
  try {
    await invoke("save_settings", { settings: s });
    S.settings = s;
    applyAppearance();
    if (langChanged) { applyLanguage(); initPanel(); renderHosts(); initPanes(); }
    scheduleUpdates();
    invoke("refresh_status").catch(() => {}); // the new check interval starts now
    if (!transparentAtLaunch && needsTransparentWindow(s)) toast(t("bg_restart"), "info");
  } catch (e) { toast(`${t("settings_title")} : ${e}`, "error"); }
}

// ---------- about ----------

function openAbout() {
  return modal(t("about"), () => aboutView(), { cls: "about-modal" });
}

/** Shared by the About window (menu) and the About page of the settings. */
function aboutView() {
  const plat = { macos: "macOS", windows: "Windows", linux: "Linux", ios: "iOS" }[S.platform] || S.platform;
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
      !isIOS() && h("button.btn.primary", { type: "button", onclick: () => checkUpdate(true) }, t("check_update").replace(/…$/, "")),
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
  document.body.dataset.platform = document.documentElement.dataset.platform = S.platform;
  document.documentElement.dataset.desktop = b.desktop;
  transparentAtLaunch = needsTransparentWindow(S.settings);
  if (isIOS()) {
    await initNative(chromeState, nativeAction, keyInput).catch((e) => console.error("native-ui", e));
    // The key bar is shown above the keyboard while a terminal has it.
    const typing = () => { document.body.classList.toggle("typing", !!document.activeElement?.closest(".xterm")); refreshChrome(); };
    document.addEventListener("focusin", typing);
    document.addEventListener("focusout", () => setTimeout(typing));
  }
  initMenubar();
  for (const [id, name] of [["toggle-sidebar", "sidebar"], ["add-folder", "folderPlus"], ["add-host", "plus"], ["clear-term", "clear"], ["split-h", "splitH"], ["split-v", "splitV"], ["toggle-panel", "panel"], ["open-settings", "settings"]]) {
    document.getElementById(id).append(icon(name, 16));
  }
  document.getElementById("toggle-sidebar").onclick = () => setLayout("sidebar");
  document.getElementById("add-folder").onclick = () => editFolder();
  document.getElementById("add-host").onclick = () => editHost();
  document.getElementById("toggle-panel").onclick = () => setLayout("right_panel");
  document.getElementById("clear-term").onclick = clearScreen;
  document.getElementById("split-h").onclick = () => split("right");
  document.getElementById("split-v").onclick = () => split("down");
  document.getElementById("open-settings").onclick = () => openSettings();
  initSyncStatus(document.getElementById("sync-status"), () => openSettings("sync"));
  // Clicking the host list or the side panel leaves the keyboard in the terminal (Settings → Terminal).
  for (const id of ["sidebar", "panel"]) document.getElementById(id).addEventListener("click", (e) => {
    if (!S.settings.terminal.keep_focus || e.detail === 0) return; // clicks made with the keyboard keep their focus
    setTimeout(() => {
      const a = document.activeElement;
      if (a === document.body || (a?.closest("#sidebar, #panel") && !a.matches("input, textarea, select, [contenteditable]"))) focusTerminal();
    });
  });
  applyLanguage();
  // On a phone the host list covers the terminal: put it away once a host is opened.
  initHosts((host, opts) => { openHost(host, opts); if (isPhone()) setLayout("sidebar", false); });
  initPanes();
  if (isIOS()) {
    // Touching the terminal puts the open drawer away.
    document.getElementById("panes").addEventListener("touchstart", () => drawer && isPhone() && setLayout(drawer, false), { passive: true });
  }
  initPanel();
  applyAppearance();
  // Show the window once the UI is built (it starts hidden with the theme background: no white flash).
  // Not in requestAnimationFrame: a hidden window never paints, so it only fired via the 3 s fallback.
  invoke("app_ready");
  listen("menu", (e) => ACTIONS[e.payload]?.());
  listen("connect-host", (e) => { const host = S.hosts.hosts.find((x) => x.id === e.payload); if (host) newTab(host); });
  listen("host-status", (e) => { S.status = e.payload; renderStatus(); });
  // Another device changed hosts or settings: reload them.
  listen("sync-changed", async (e) => {
    if (e.payload.hosts) { S.hosts = await invoke("get_hosts"); document.dispatchEvent(new Event("hosts-changed")); }
    if (e.payload.settings) {
      const langChanged = (await invoke("boot").then((b) => { const c = b.settings.appearance.language !== S.settings.appearance.language; S.settings = b.settings; return c; }));
      applyAppearance();
      if (langChanged) { applyLanguage(); initPanel(); renderHosts(); initPanes(); }
    }
  });
  if (S.settings.behavior.check_updates) { setTimeout(() => checkUpdate(false), 2000); scheduleUpdates(); }
}

boot().catch((e) => {
  document.body.replaceChildren(h("pre.error", {}, t("boot_error", e)));
  invoke("app_ready");
});
