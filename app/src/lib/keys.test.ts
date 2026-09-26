import { describe, expect, it } from "vitest";
import { ChordRecorder, keyName } from "./keys";

const ev = (code: string, key = "") => ({ code, key });

describe("keyName", () => {
  it("maps physical codes to Aural key names", () => {
    expect(keyName(ev("ControlLeft"))).toBe("Ctrl");
    expect(keyName(ev("ControlRight"))).toBe("RightCtrl");
    expect(keyName(ev("MetaLeft"))).toBe("Win");
    expect(keyName(ev("AltRight"))).toBe("RightAlt");
    expect(keyName(ev("ShiftLeft"))).toBe("Shift");
    expect(keyName(ev("KeyK"))).toBe("K");
    expect(keyName(ev("Digit7"))).toBe("7");
    expect(keyName(ev("F9"))).toBe("F9");
    expect(keyName(ev("Space"))).toBe("Space");
    expect(keyName(ev("Unidentified"))).toBeNull();
  });
});

describe("ChordRecorder", () => {
  it("records the full set of keys held together, reported when all are released", () => {
    const r = new ChordRecorder();
    expect(r.down(ev("ControlLeft"))).toBeNull();
    expect(r.down(ev("MetaLeft"))).toBeNull();
    expect(r.up(ev("MetaLeft"))).toBeNull();
    expect(r.up(ev("ControlLeft"))).toEqual(["Ctrl", "Win"]);
  });

  it("orders modifiers first regardless of press order", () => {
    const r = new ChordRecorder();
    r.down(ev("Space"));
    r.down(ev("ShiftLeft"));
    r.down(ev("ControlLeft"));
    r.up(ev("Space"));
    r.up(ev("ShiftLeft"));
    expect(r.up(ev("ControlLeft"))).toEqual(["Ctrl", "Shift", "Space"]);
  });

  it("ignores key repeat and unknown keys", () => {
    const r = new ChordRecorder();
    r.down(ev("F9"));
    r.down(ev("F9"));
    r.down(ev("Unidentified"));
    expect(r.up(ev("F9"))).toEqual(["F9"]);
  });

  it("starts fresh after each chord", () => {
    const r = new ChordRecorder();
    r.down(ev("F9"));
    r.up(ev("F9"));
    r.down(ev("ControlRight"));
    expect(r.up(ev("ControlRight"))).toEqual(["RightCtrl"]);
  });
});
