/** The exact phrase the user must type to delete Aural. The backend re-checks it. */
export const DELETE_PHRASE = "YES, DELETE";

/** Exact match only: no trimming, no case folding, so nothing confirms by accident. */
export function isDeleteConfirmed(input: string): boolean {
  return input === DELETE_PHRASE;
}
