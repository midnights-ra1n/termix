// Fonts bundled with the app (no network): the interface (sans) and terminal (mono) pickers list these.
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/500.css";
import "@fontsource/geist-sans/600.css";
import "@fontsource/source-sans-3/400.css";
import "@fontsource/source-sans-3/500.css";
import "@fontsource/source-sans-3/600.css";
import "@fontsource/atkinson-hyperlegible/400.css";
import "@fontsource/atkinson-hyperlegible/700.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/ibm-plex-mono/600.css";
import "@fontsource/ibm-plex-mono/700.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/700.css";
import "@fontsource/fira-code/400.css";
import "@fontsource/fira-code/500.css";
import "@fontsource/fira-code/700.css";
import "@fontsource/geist-mono/400.css";
import "@fontsource/geist-mono/500.css";
import "@fontsource/geist-mono/700.css";
import "@fontsource/source-code-pro/400.css";
import "@fontsource/source-code-pro/500.css";
import "@fontsource/source-code-pro/700.css";
import "@fontsource/cascadia-code/400.css";
import "@fontsource/cascadia-code/500.css";
import "@fontsource/cascadia-code/700.css";

const SANS_FALLBACK = "system-ui, sans-serif";
const MONO_FALLBACK = "Menlo, Consolas, monospace";

/** [label, CSS font stack]; the first one is the default (same strings as config.rs). */
export const UI_FONTS = [
  ...["IBM Plex Sans", "Inter", "Geist Sans", "Source Sans 3", "Atkinson Hyperlegible"].map((f) => [f, `"${f}", ${SANS_FALLBACK}`]),
  ["System", "system-ui, -apple-system, \"Segoe UI\", Cantarell, sans-serif"],
];
export const TERM_FONTS = [
  ...["IBM Plex Mono", "JetBrains Mono", "Fira Code", "Geist Mono", "Source Code Pro", "Cascadia Code"].map((f) => [f, `"${f}", ${MONO_FALLBACK}`]),
  ["System", "ui-monospace, \"SF Mono\", Menlo, Consolas, \"DejaVu Sans Mono\", monospace"],
];
