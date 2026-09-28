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

/** Live words only count while the user is speaking. An update with no words (a new
 * phrase or stream starting) keeps what is shown, so the caption doesn't blink. */
export function captionAfterLive(shown: LiveText, words: LiveText, kind: Kind): LiveText {
  if (kind !== "listening") return shown;
  if (words.stable.trim() === "" && words.tentative.trim() === "") return shown;
  return words;
}

export interface CaptionWord {
  text: string;
  /** Finished (white) rather than still able to change (grey). */
  stable: boolean;
}

/** The caption as words in reading order. Drawn keyed by position, so a word already on
 * screen stays put (and only changes its text if corrected) while new words fade in. */
export function captionWords(words: LiveText): CaptionWord[] {
  const split = (s: string) => s.split(/\s+/).filter((w) => w !== "");
  return [
    ...split(words.stable).map((text) => ({ text, stable: true })),
    ...split(words.tentative).map((text) => ({ text, stable: false })),
  ];
}
