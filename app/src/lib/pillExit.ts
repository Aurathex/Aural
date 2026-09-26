import type { PillView } from "./types";

/** What the pill draws: the content, and whether it is fading out. */
export interface PillDisplay {
  content: PillView;
  leaving: boolean;
}

/**
 * When the pill is told to hide, keep drawing what it last showed (the success line,
 * an error label) while it fades, instead of swapping to empty dots mid-fade. A new
 * visible state cancels the fade.
 */
export function nextDisplay(prev: PillDisplay, incoming: PillView): PillDisplay {
  if (incoming.state.state !== "hidden") return { content: incoming, leaving: false };
  if (prev.content.state.state === "hidden") return prev;
  return { content: prev.content, leaving: true };
}
