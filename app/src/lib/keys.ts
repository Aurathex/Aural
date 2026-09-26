/** Key names shared with the Rust side (aural-platform `Chord::parse`). */
const CODES: Record<string, string> = {
  ControlLeft: "Ctrl",
  ControlRight: "RightCtrl",
  AltLeft: "Alt",
  AltRight: "RightAlt",
  ShiftLeft: "Shift",
  ShiftRight: "RightShift",
  MetaLeft: "Win",
  MetaRight: "Win",
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Pause: "Pause",
  ScrollLock: "ScrollLock",
};

const MODIFIER_ORDER = ["Ctrl", "RightCtrl", "Alt", "RightAlt", "Shift", "RightShift", "Win"];

export function keyName(e: { code: string }): string | null {
  const named = CODES[e.code];
  if (named) return named;
  let m = /^Key([A-Z])$/.exec(e.code);
  if (m) return m[1]!;
  m = /^Digit([0-9])$/.exec(e.code);
  if (m) return m[1]!;
  m = /^F([1-9]|1[0-9]|2[0-4])$/.exec(e.code);
  if (m) return e.code;
  return null;
}

/** Collects the keys held together and reports the chord once every key is up. */
export class ChordRecorder {
  private held = new Set<string>();
  private seen: string[] = [];

  down(e: { code: string }): null {
    const k = keyName(e);
    if (!k) return null;
    this.held.add(k);
    if (!this.seen.includes(k)) this.seen.push(k);
    return null;
  }

  up(e: { code: string }): string[] | null {
    const k = keyName(e);
    if (k) this.held.delete(k);
    if (this.held.size > 0 || this.seen.length === 0) return null;
    const mods = MODIFIER_ORDER.filter((m) => this.seen.includes(m));
    const rest = this.seen.filter((k2) => !MODIFIER_ORDER.includes(k2));
    this.seen = [];
    return [...mods, ...rest];
  }

  reset(): void {
    this.held.clear();
    this.seen = [];
  }
}
