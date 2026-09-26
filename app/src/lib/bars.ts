/** Twelve level bands arrive from the microphone ~30 times a second. */
export const BAR_COUNT = 12;

const ATTACK_MS = 25;
const RELEASE_MS = 160;

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/**
 * One-pole smoothing per band: quick to rise so syllables feel immediate, slow to fall
 * so the bars breathe instead of flicker.
 */
export function smooth(prev: readonly number[], target: readonly number[], dtMs: number): number[] {
  return target.map((t, i) => {
    const goal = clamp01(t);
    const p = prev[i] ?? 0;
    const tau = goal > p ? ATTACK_MS : RELEASE_MS;
    const alpha = 1 - Math.exp(-Math.max(0, dtMs) / tau);
    return clamp01(p + (goal - p) * alpha);
  });
}

/**
 * Bar heights in pixels. Bands are mirrored from the centre outward (lowest bands in
 * the middle, where most speech energy is), alternating left and right, so a voice
 * swells from the centre of the pill.
 */
export function barHeights(levels: readonly number[], maxPx: number, minPx: number): number[] {
  const half = BAR_COUNT / 2;
  return Array.from({ length: BAR_COUNT }, (_, pos) => {
    const left = pos < half;
    const distance = left ? half - 1 - pos : pos - half;
    const band = distance * 2 + (left ? 0 : 1);
    const level = clamp01(levels[band] ?? 0);
    return Math.round(minPx + (maxPx - minPx) * level);
  });
}
