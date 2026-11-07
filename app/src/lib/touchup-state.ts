import type { TouchupPreview } from "./types";

/** Merge independently delivered snapshots and command results for one preview. */
export function latestTouchup(current: TouchupPreview | undefined, incoming: TouchupPreview): TouchupPreview {
  if (!current) return incoming;
  if (incoming.id !== current.id) return incoming.id > current.id ? incoming : current;
  const phaseOrder: Record<TouchupPreview["phase"], number> = {
    "": 0, working: 1, review: 2, applying: 3, done: 4, error: 4,
  };
  return phaseOrder[incoming.phase] < phaseOrder[current.phase] ? current : incoming;
}
