import { describe, expect, it } from "vitest";
import { nextDisplay, type PillDisplay } from "./pillExit";
import type { PillView } from "./types";

const v = (state: PillView["state"], label: string | null = null): PillView => ({ state, label });
const start: PillDisplay = { content: v({ state: "hidden" }), leaving: false };

describe("pill exit", () => {
  it("shows each visible state as it arrives", () => {
    const d = nextDisplay(start, v({ state: "listening" }));
    expect(d).toEqual({ content: v({ state: "listening" }), leaving: false });
  });

  it("keeps the last visible content while fading out", () => {
    const success = nextDisplay(start, v({ state: "success" }));
    const leaving = nextDisplay(success, v({ state: "hidden" }));
    expect(leaving.leaving).toBe(true);
    expect(leaving.content.state.state).toBe("success");
  });

  it("keeps an error label readable while it fades", () => {
    const err = nextDisplay(start, v({ state: "error", error: "no_model" }, "No model"));
    const leaving = nextDisplay(err, v({ state: "hidden" }));
    expect(leaving.content.label).toBe("No model");
  });

  it("a new session during the fade cancels it", () => {
    const leaving = nextDisplay(nextDisplay(start, v({ state: "success" })), v({ state: "hidden" }));
    const back = nextDisplay(leaving, v({ state: "listening" }));
    expect(back).toEqual({ content: v({ state: "listening" }), leaving: false });
  });

  it("hidden before anything was shown stays hidden without an exit", () => {
    expect(nextDisplay(start, v({ state: "hidden" }))).toEqual(start);
  });
});
