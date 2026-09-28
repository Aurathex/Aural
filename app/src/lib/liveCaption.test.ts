import { describe, expect, it } from "vitest";
import { captionAfterLive, captionAfterPill, captionWords, EMPTY } from "./liveCaption";
import type { PillView } from "./types";

const view = (state: PillView["state"]["state"]): PillView =>
  ({ state: state === "error" ? { state, error: "x" } : { state }, label: null }) as PillView;

describe("live caption", () => {
  it("starts empty for each new recording", () => {
    const shown = { stable: "old words", tentative: "" };
    expect(captionAfterPill(shown, view("listening"), "hidden")).toEqual(EMPTY);
  });

  it("keeps the words while the final text is being prepared", () => {
    const shown = { stable: "keep", tentative: "me" };
    expect(captionAfterPill(shown, view("processing"), "listening")).toEqual(shown);
    // A repeated listening update within the same recording keeps them too.
    expect(captionAfterPill(shown, view("listening"), "listening")).toEqual(shown);
  });

  it("clears once the text is inserted, fails or the pill hides", () => {
    const shown = { stable: "done", tentative: "" };
    for (const s of ["success", "error", "hidden"] as const) {
      expect(captionAfterPill(shown, view(s), "processing")).toEqual(EMPTY);
    }
  });

  it("an empty update while speaking does not blank the caption (no flicker)", () => {
    const shown = { stable: "so the meeting", tentative: "is at" };
    // A new phrase or stream starting reports nothing for a moment.
    expect(captionAfterLive(shown, EMPTY, "listening")).toEqual(shown);
    expect(captionAfterLive(shown, { stable: "", tentative: " " }, "listening")).toEqual(shown);
    // Real words still replace the caption, including corrections.
    const next = { stable: "so the meeting is", tentative: "at three" };
    expect(captionAfterLive(shown, next, "listening")).toEqual(next);
  });

  it("splits the caption into words, finished ones first, keyed by position", () => {
    expect(captionWords({ stable: "so the", tentative: "meeting is" })).toEqual([
      { text: "so", stable: true },
      { text: "the", stable: true },
      { text: "meeting", stable: false },
      { text: "is", stable: false },
    ]);
    expect(captionWords(EMPTY)).toEqual([]);
    expect(captionWords({ stable: "  one  ", tentative: "" })).toEqual([{ text: "one", stable: true }]);
  });

  it("ignores late words once the recording is over", () => {
    const late = { stable: "late", tentative: "" };
    expect(captionAfterLive(EMPTY, late, "success")).toEqual(EMPTY);
    expect(captionAfterLive(EMPTY, late, "hidden")).toEqual(EMPTY);
    expect(captionAfterLive(EMPTY, late, "listening")).toEqual(late);
  });
});
