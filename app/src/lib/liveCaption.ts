import type { LiveText, PillView } from "./types";

export const EMPTY: LiveText = { stable: "", tentative: "" };

type Kind = PillView["state"]["state"];

/** The caption after the pill changes state: fresh for a new recording, kept while the
 * final text is prepared, gone once it's inserted or dictation ends. */
export function captionAfterPill(shown: LiveText, next: PillView, prev: Kind): LiveText {
  const kind = next.state.state;
  if (kind === "listening") return prev === "listening" ? shown : EMPTY;
  if (kind === "processing") return shown;
  return EMPTY;
}

/** Live words only count while the user is speaking. */
export function captionAfterLive(shown: LiveText, words: LiveText, kind: Kind): LiveText {
  return kind === "listening" ? words : shown;
}
