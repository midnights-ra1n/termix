// Built-in themes. Each one sets UI tokens + the xterm ANSI palette.
// User themes from config.toml [themes.<name>] use the same keys and override/extend these.

const ansi = (s) => {
  const k = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"];
  return Object.fromEntries(s.split(" ").map((c, i) => [k[i], c]));
};

export const THEMES = {
  midnights: {
    label: "Midnights (nuit)", dark: true,
    bg: "#0B1130", scene: "#121A42", vide: "#060B1F", indigo: "#1A2558", brume: "#263478",
    ink: "#EEEBFF", mute: "#BDB6EC", faint: "#9A92D6",
    accent: "#C3BAFE", accent2: "#FFB0CF", success: "#91E3AE", error: "#FB909E", warn: "#FFBE8D", onAccent: "#0B1130",
    cursor: "#FFB0CF", selection: "#263478",
    ...ansi("#1A2558 #FB909E #91E3AE #FFBE8D #87D7F7 #E09FEA #87D7F7 #BDB6EC #585B8A #FFB0CF #91E3AE #FFBE8D #C3BAFE #E09FEA #87D7F7 #EEEBFF"),
  },
  aube: {
    label: "Midnights (aube)", dark: false,
    bg: "#F5F5FE", scene: "#FDFDFF", vide: "#EAEAF9", indigo: "#EAEAF9", brume: "#D8D8EE",
    ink: "#0B1130", mute: "#404468", faint: "#585B8A",
    accent: "#5F47AE", accent2: "#9F2464", success: "#04703F", error: "#AF0040", warn: "#8C4A01", onAccent: "#FDFDFF",
    cursor: "#9F2464", selection: "#D8D8EE",
    ...ansi("#0B1130 #AF0040 #04703F #8C4A01 #056883 #8E229E #056883 #404468 #585B8A #AF0040 #04703F #8C4A01 #5F47AE #8E229E #056883 #0B1130"),
  },
  nord: {
    label: "Nord", dark: true,
    bg: "#2E3440", scene: "#3B4252", vide: "#272C36", indigo: "#434C5E", brume: "#4C566A",
    ink: "#ECEFF4", mute: "#D8DEE9", faint: "#A3ABBA",
    accent: "#88C0D0", accent2: "#B48EAD", success: "#A3BE8C", error: "#BF616A", warn: "#EBCB8B", onAccent: "#2E3440",
    cursor: "#D8DEE9", selection: "#4C566A",
    ...ansi("#3B4252 #BF616A #A3BE8C #EBCB8B #81A1C1 #B48EAD #88C0D0 #E5E9F0 #4C566A #BF616A #A3BE8C #EBCB8B #81A1C1 #B48EAD #8FBCBB #ECEFF4"),
  },
  dracula: {
    label: "Dracula", dark: true,
    bg: "#282A36", scene: "#303341", vide: "#21222C", indigo: "#383A4A", brume: "#44475A",
    ink: "#F8F8F2", mute: "#D6D6E0", faint: "#A4AAD0",
    accent: "#BD93F9", accent2: "#FF79C6", success: "#50FA7B", error: "#FF5555", warn: "#FFB86C", onAccent: "#282A36",
    cursor: "#F8F8F2", selection: "#44475A",
    ...ansi("#21222C #FF5555 #50FA7B #F1FA8C #BD93F9 #FF79C6 #8BE9FD #F8F8F2 #6272A4 #FF6E6E #69FF94 #FFFFA5 #D6ACFF #FF92DF #A4FFFF #FFFFFF"),
  },
  solarized: {
    label: "Solarized Dark", dark: true,
    bg: "#002B36", scene: "#073642", vide: "#00212B", indigo: "#0A4250", brume: "#145A6A",
    ink: "#EEE8D5", mute: "#93A1A1", faint: "#839496",
    accent: "#268BD2", accent2: "#D33682", success: "#859900", error: "#DC322F", warn: "#B58900", onAccent: "#FDF6E3",
    cursor: "#93A1A1", selection: "#145A6A",
    ...ansi("#073642 #DC322F #859900 #B58900 #268BD2 #D33682 #2AA198 #EEE8D5 #586E75 #CB4B16 #93A1A1 #839496 #839496 #6C71C4 #93A1A1 #FDF6E3"),
  },
  gruvbox: {
    label: "Gruvbox Dark", dark: true,
    bg: "#282828", scene: "#32302F", vide: "#1D2021", indigo: "#3C3836", brume: "#504945",
    ink: "#FBF1C7", mute: "#D5C4A1", faint: "#BDAE93",
    accent: "#FABD2F", accent2: "#D3869B", success: "#B8BB26", error: "#FB4934", warn: "#FE8019", onAccent: "#282828",
    cursor: "#EBDBB2", selection: "#504945",
    ...ansi("#282828 #CC241D #98971A #D79921 #458588 #B16286 #689D6A #A89984 #928374 #FB4934 #B8BB26 #FABD2F #83A598 #D3869B #8EC07C #EBDBB2"),
  },
  // Follows the OS: system colours, accent colour and light/dark mode.
  system: { label: "Système (natif)", system: true },
};

function systemTheme() {
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  const probe = document.createElement("div");
  document.body.append(probe);
  const read = (c) => { probe.style.color = c; return getComputedStyle(probe).color; };
  const accent = read("AccentColor") || "#0A84FF";
  probe.remove();
  const base = dark ? THEMES.nord : THEMES.aube;
  return dark
    ? { ...base, bg: "#1E1E1E", scene: "#262626", vide: "#161616", indigo: "#2E2E2E", brume: "#3A3A3A", ink: "#F2F2F2", mute: "#C8C8C8", faint: "#9A9A9A", accent, accent2: accent, cursor: accent, selection: "#3A3A3A", black: "#2E2E2E", brightWhite: "#FFFFFF" }
    : { ...base, bg: "#F6F6F6", scene: "#FFFFFF", vide: "#ECECEC", indigo: "#EBEBEB", brume: "#D6D6D6", ink: "#1D1D1F", mute: "#4A4A4F", faint: "#6E6E73", accent, accent2: accent, cursor: accent, selection: "#D6D6D6" };
}

export function resolveTheme(name, userThemes = {}) {
  const all = { ...THEMES, ...userThemes };
  let t = all[name] || THEMES.midnights;
  if (t.system) t = systemTheme();
  else if (userThemes[name]) t = { ...(THEMES[t.base] || THEMES.midnights), ...t };
  return t;
}

export function applyTheme(t) {
  const r = document.documentElement.style;
  for (const k of ["bg", "scene", "vide", "indigo", "brume", "ink", "mute", "faint", "accent", "accent2", "success", "error", "warn", "onAccent"]) {
    if (t[k]) r.setProperty(`--${k}`, t[k]);
  }
  document.documentElement.dataset.dark = t.dark === false ? "0" : "1";
}

export function xtermTheme(t) {
  const { bg, ink, cursor, selection } = t;
  const keys = Object.keys(ansi("a b c d e f g h i j k l m n o p"));
  return {
    background: bg, foreground: ink, cursor, cursorAccent: bg, selectionBackground: selection,
    ...Object.fromEntries(keys.filter((k) => t[k]).map((k) => [k, t[k]])),
  };
}
