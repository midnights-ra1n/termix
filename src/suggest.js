// Inline command prediction (ghost text) + completion list, fed by the remote shell history and local history.
// The current command is read from the screen (not from keystrokes) so it stays right after Tab or ↑ in the remote shell.

const PROMPT_END = /[$#%>❯»➜λ](?=\s)/u;

/** Text typed after the prompt, or null when the line has no recognisable prompt. */
export function extractCommand(left) {
  const m = PROMPT_END.exec(left);
  if (!m) return null;
  return left.slice(m.index + m[0].length).replace(/^\s+/, "");
}

/** Most recent history entries that extend `prefix`, newest first, unique. */
export function candidates(prefix, history, limit = 8) {
  const out = [];
  if (!prefix.trim()) return out;
  for (let i = history.length - 1; i >= 0 && out.length < limit; i--) {
    const c = history[i];
    if (c.length > prefix.length && c.startsWith(prefix) && !out.includes(c)) out.push(c);
  }
  return out;
}

/** Next word of a suggestion suffix (for Alt+→). */
export const nextWord = (suffix) => suffix.match(/^\s*\S+/)?.[0] ?? suffix;

const key = (hostId) => "hist:" + hostId;
function loadLocal(hostId) { try { return JSON.parse(localStorage.getItem(key(hostId)) || "[]"); } catch { return []; } }
function saveLocal(hostId, list) { try { localStorage.setItem(key(hostId), JSON.stringify(list.slice(-1000))); } catch {} }
export function clearLocalHistory() {
  try { Object.keys(localStorage).filter((k) => k.startsWith("hist:")).forEach((k) => localStorage.removeItem(k)); } catch {}
}

export class Suggest {
  constructor(term, hostId, send, isEnabled) {
    Object.assign(this, { term, hostId, send, isEnabled, remote: [], local: loadLocal(hostId), cmd: null, suffix: "", list: null, sel: 0 });
    this.ghost = document.createElement("div");
    this.ghost.className = "ghost";
    this.ghost.setAttribute("aria-hidden", "true");
    this.menu = document.createElement("ul");
    this.menu.className = "suggest-list";
    this.menu.setAttribute("role", "listbox");
    this.ghost.hidden = this.menu.hidden = true; // shown on demand; an empty list showed as a bar under the prompt
    term.element.append(this.ghost, this.menu);
    let raf = 0;
    const schedule = () => { cancelAnimationFrame(raf); raf = requestAnimationFrame(() => this.update()); };
    this.subs = [term.onWriteParsed(schedule), term.onCursorMove(schedule), term.onScroll(schedule), term.onResize(schedule), term.onSelectionChange(schedule)];
  }

  setRemote(list) { this.remote = list; this.update(); }
  history() { return this.remote.concat(this.local); }

  /** Reads the command under the cursor; null when suggestions must not show. */
  current() {
    const t = this.term, b = t.buffer.active;
    if (!this.isEnabled() || b.type !== "normal" || t.hasSelection() || b.viewportY !== b.baseY) return null;
    let y = b.baseY + b.cursorY;
    const line = b.getLine(y);
    if (!line || line.translateToString(true, b.cursorX).trim()) return null; // text right of the cursor
    let left = line.translateToString(false, 0, b.cursorX);
    for (let i = 0; i < 4 && b.getLine(y)?.isWrapped; i++) left = b.getLine(--y).translateToString(false) + left;
    return extractCommand(left);
  }

  update() {
    this.cmd = this.current();
    const list = this.cmd ? candidates(this.cmd, this.history()) : [];
    if (this.list && !list.length) this.closeList();
    if (this.list) { this.list = list; this.sel = Math.min(this.sel, list.length - 1); this.renderList(); }
    this.suffix = list[0] ? list[0].slice(this.cmd.length) : "";
    this.renderGhost();
  }

  cell() {
    const screen = this.term.element.querySelector(".xterm-screen");
    return { w: screen.clientWidth / this.term.cols, h: screen.clientHeight / this.term.rows };
  }

  renderGhost() {
    const g = this.ghost, b = this.term.buffer.active;
    if (!this.suffix || this.list) { g.hidden = true; return; }
    const { w, h } = this.cell();
    const room = this.term.cols - b.cursorX;
    g.textContent = this.suffix.length > room ? this.suffix.slice(0, room) : this.suffix;
    Object.assign(g.style, {
      left: `${b.cursorX * w}px`, top: `${b.cursorY * h}px`, height: `${h}px`, lineHeight: `${h}px`,
      fontFamily: this.term.options.fontFamily, fontSize: `${this.term.options.fontSize}px`,
    });
    g.hidden = false;
  }

  openList() {
    const list = this.cmd ? candidates(this.cmd, this.history()) : [];
    if (!list.length) return false;
    this.list = list;
    this.sel = 0;
    this.renderList();
    this.renderGhost();
    return true;
  }
  closeList() { this.list = null; this.menu.hidden = true; this.renderGhost(); }

  renderList() {
    const b = this.term.buffer.active, { w, h } = this.cell();
    this.menu.replaceChildren(...this.list.map((c, i) => {
      const li = document.createElement("li");
      li.setAttribute("role", "option");
      li.setAttribute("aria-selected", String(i === this.sel));
      li.className = i === this.sel ? "on" : "";
      const typed = document.createElement("b");
      typed.textContent = this.cmd;
      li.append(typed, c.slice(this.cmd.length));
      li.onmousedown = (e) => { e.preventDefault(); this.sel = i; this.acceptList(); };
      return li;
    }));
    const below = b.cursorY < this.term.rows / 2;
    Object.assign(this.menu.style, {
      left: `${Math.max(0, b.cursorX - this.cmd.length) * w}px`,
      top: below ? `${(b.cursorY + 1) * h + 4}px` : "", bottom: below ? "" : `${(this.term.rows - b.cursorY) * h + 4}px`,
      fontFamily: this.term.options.fontFamily,
    });
    this.menu.hidden = false;
  }

  acceptList() {
    const pick = this.list?.[this.sel];
    if (pick) this.send(pick.slice(this.cmd.length));
    this.closeList();
  }

  /** Returns false when the key was consumed (xterm must not send it). */
  handleKey(e) {
    if (e.type !== "keydown") return true;
    if (this.list) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") { this.sel = (this.sel + (e.key === "ArrowDown" ? 1 : -1) + this.list.length) % this.list.length; this.renderList(); return false; }
      if (e.key === "Enter" || e.key === "Tab" || e.key === "ArrowRight") { e.preventDefault(); this.acceptList(); return false; }
      if (e.key === "Escape") { this.closeList(); return false; }
      this.closeList();
    }
    // ⇧Tab opens the list (Ctrl+Space is the input-source switch on macOS).
    if (e.key === "Tab" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && this.cmd) { e.preventDefault(); return !this.openList(); }
    const plain = !e.ctrlKey && !e.metaKey && !e.shiftKey;
    if (this.suffix && plain && (e.key === "ArrowRight" || e.key === "End") && !e.altKey) { this.send(this.suffix); return false; }
    if (this.suffix && plain && e.altKey && e.key === "ArrowRight") { this.send(nextWord(this.suffix)); return false; }
    if (e.key === "Enter" && !e.shiftKey) this.record();
    return true;
  }

  record() {
    const cmd = this.current()?.trim();
    if (!cmd || cmd.length < 2) return;
    this.local = this.local.filter((c) => c !== cmd);
    this.local.push(cmd);
    saveLocal(this.hostId, this.local);
  }

  dispose() { this.subs.forEach((s) => s.dispose()); this.ghost.remove(); this.menu.remove(); }
}
