// Built-in themes. Each one sets UI tokens + the xterm ANSI palette.
// User themes from config.toml [themes.<name>] use the same keys and override/extend these.

const ansi = (s) => {
  const k = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"];
  return Object.fromEntries(s.split(" ").map((c, i) => [k[i], c]));
};

export const THEMES = {
  termix: {
    label: "Termix", dark: true,
    bg: "#36415A", scene: "#3E4A64", vide: "#2B3448", indigo: "#46546F", brume: "#55647F",
    ink: "#EEEEEE", mute: "#CDD3DD", faint: "#B3BBC8",
    accent: "#5D866C", accent2: "#9CCBA9", success: "#9CCBA9", error: "#F0A0A0", warn: "#EBC48A", onAccent: "#FFFFFF",
    // The accent is too dark against the background for a focus ring (3:1): use its light tint.
    focus: "#9CCBA9",
    cursor: "#9CCBA9", selection: "#55647F",
    ...ansi("#2B3448 #F0A0A0 #9CCBA9 #EBC48A #9DC0EC #C9AEE6 #8FD0CF #CDD3DD #8693AA #F5B5B5 #B5DBC0 #F2D5A6 #B6D0F2 #D9C4EE #ACDEDD #F5F5F5"),
  },
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
  catppuccin: {
    label: "Catppuccin Mocha", dark: true,
    bg: "#1E1E2E", scene: "#181825", vide: "#11111B", indigo: "#313244", brume: "#45475A",
    ink: "#CDD6F4", mute: "#BAC2DE", faint: "#9399B2",
    accent: "#CBA6F7", accent2: "#F5C2E7", success: "#A6E3A1", error: "#F38BA8", warn: "#FAB387", onAccent: "#1E1E2E",
    cursor: "#F5E0DC", selection: "#45475A",
    ...ansi("#45475A #F38BA8 #A6E3A1 #F9E2AF #89B4FA #F5C2E7 #94E2D5 #BAC2DE #585B70 #F38BA8 #A6E3A1 #F9E2AF #89B4FA #F5C2E7 #94E2D5 #A6ADC8"),
  },
  latte: {
    label: "Catppuccin Latte", dark: false,
    bg: "#EFF1F5", scene: "#E6E9EF", vide: "#DCE0E8", indigo: "#DCE0E8", brume: "#BCC0CC",
    ink: "#4C4F69", mute: "#55586F", faint: "#606378",
    accent: "#8839EF", accent2: "#D20F39", success: "#40A02B", error: "#D20F39", warn: "#C04A06", onAccent: "#EFF1F5",
    cursor: "#DC8A78", selection: "#CCD0DA",
    ...ansi("#5C5F77 #D20F39 #40A02B #DF8E1D #1E66F5 #EA76CB #179299 #ACB0BE #6C6F85 #D20F39 #40A02B #DF8E1D #1E66F5 #EA76CB #179299 #BCC0CC"),
  },
  tokyonight: {
    label: "Tokyo Night", dark: true,
    bg: "#1A1B26", scene: "#16161E", vide: "#13131A", indigo: "#292E42", brume: "#3B4261",
    ink: "#C0CAF5", mute: "#A9B1D6", faint: "#8089B3",
    accent: "#7AA2F7", accent2: "#BB9AF7", success: "#9ECE6A", error: "#F7768E", warn: "#E0AF68", onAccent: "#1A1B26",
    cursor: "#C0CAF5", selection: "#33467C",
    ...ansi("#15161E #F7768E #9ECE6A #E0AF68 #7AA2F7 #BB9AF7 #7DCFFF #A9B1D6 #414868 #F7768E #9ECE6A #E0AF68 #7AA2F7 #BB9AF7 #7DCFFF #C0CAF5"),
  },
  onedark: {
    label: "One Dark", dark: true,
    bg: "#282C34", scene: "#21252B", vide: "#1B1F23", indigo: "#2C313A", brume: "#3E4451",
    ink: "#DCDFE4", mute: "#ABB2BF", faint: "#959BA6",
    accent: "#61AFEF", accent2: "#C678DD", success: "#98C379", error: "#E06C75", warn: "#E5C07B", onAccent: "#282C34",
    cursor: "#528BFF", selection: "#3E4451",
    ...ansi("#282C34 #E06C75 #98C379 #E5C07B #61AFEF #C678DD #56B6C2 #ABB2BF #5C6370 #E06C75 #98C379 #E5C07B #61AFEF #C678DD #56B6C2 #FFFFFF"),
  },
  rosepine: {
    label: "Rosé Pine", dark: true,
    bg: "#191724", scene: "#1F1D2E", vide: "#13111E", indigo: "#26233A", brume: "#403D52",
    ink: "#E0DEF4", mute: "#B9B5D1", faint: "#908CAA",
    accent: "#C4A7E7", accent2: "#EBBCBA", success: "#9CCFD8", error: "#EB6F92", warn: "#F6C177", onAccent: "#191724",
    cursor: "#EBBCBA", selection: "#403D52",
    ...ansi("#26233A #EB6F92 #31748F #F6C177 #9CCFD8 #C4A7E7 #EBBCBA #E0DEF4 #6E6A86 #EB6F92 #31748F #F6C177 #9CCFD8 #C4A7E7 #EBBCBA #E0DEF4"),
  },
  everforest: {
    label: "Everforest", dark: true,
    bg: "#2D353B", scene: "#343F44", vide: "#232A2E", indigo: "#3D484D", brume: "#475258",
    ink: "#D3C6AA", mute: "#B8B49E", faint: "#A8B2AB",
    accent: "#A7C080", accent2: "#D699B6", success: "#A7C080", error: "#E67E80", warn: "#DBBC7F", onAccent: "#2D353B",
    cursor: "#D3C6AA", selection: "#475258",
    ...ansi("#343F44 #E67E80 #A7C080 #DBBC7F #7FBBB3 #D699B6 #83C092 #D3C6AA #5C6A72 #E67E80 #A7C080 #DBBC7F #7FBBB3 #D699B6 #83C092 #D3C6AA"),
  },
  kanagawa: {
    label: "Kanagawa", dark: true,
    bg: "#1F1F28", scene: "#2A2A37", vide: "#16161D", indigo: "#363646", brume: "#54546D",
    ink: "#DCD7BA", mute: "#C8C093", faint: "#9E9B87",
    accent: "#7E9CD8", accent2: "#D27E99", success: "#98BB6C", error: "#E46876", warn: "#FFA066", onAccent: "#1F1F28",
    cursor: "#C8C093", selection: "#2D4F67",
    ...ansi("#16161D #C34043 #76946A #C0A36E #7E9CD8 #957FB8 #6A9589 #C8C093 #727169 #E82424 #98BB6C #E6C384 #7FB4CA #938AA9 #7AA89F #DCD7BA"),
  },
  github: {
    label: "GitHub Light", dark: false,
    bg: "#FFFFFF", scene: "#F6F8FA", vide: "#EAEEF2", indigo: "#EAEEF2", brume: "#D0D7DE",
    ink: "#1F2328", mute: "#424A53", faint: "#59636E",
    accent: "#0969DA", accent2: "#BF3989", success: "#1A7F37", error: "#CF222E", warn: "#9A6700", onAccent: "#FFFFFF",
    cursor: "#0969DA", selection: "#B6E3FF",
    ...ansi("#24292F #CF222E #116329 #4D2D00 #0969DA #8250DF #1B7C83 #6E7781 #57606A #A40E26 #1A7F37 #633C01 #218BFF #A475F9 #3192AA #8C959F"),
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
  let t = all[name] || THEMES.termix;
  if (t.system) t = systemTheme();
  else if (userThemes[name]) t = { ...(THEMES[t.base] || THEMES.termix), ...t };
  return t;
}

export function applyTheme(t) {
  const r = document.documentElement.style;
  for (const k of ["bg", "scene", "vide", "indigo", "brume", "ink", "mute", "faint", "accent", "accent2", "success", "error", "warn", "onAccent"]) {
    if (t[k]) r.setProperty(`--${k}`, t[k]);
  }
  r.setProperty("--focus", t.focus || t.accent);
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
