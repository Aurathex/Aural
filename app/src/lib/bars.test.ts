import { describe, expect, it } from "vitest";
import { BAR_COUNT, barHeights, smooth } from "./bars";

describe("level smoothing", () => {
  it("rises fast (attack) and falls slowly (release)", () => {
    const up = smooth([0], [1], 33)[0]!;
    const down = smooth([1], [0], 33)[0]!;
    expect(up).toBeGreaterThan(0.6);
    expect(1 - down).toBeLessThan(0.3);
  });

  it("converges to the target", () => {
    let v = [0];
    for (let i = 0; i < 60; i++) v = smooth(v, [0.5], 33);
    expect(v[0]).toBeCloseTo(0.5, 3);
  });

  it("keeps values in 0..1", () => {
    expect(smooth([0.5], [7], 33)[0]).toBeLessThanOrEqual(1);
    expect(smooth([0.5], [-3], 33)[0]).toBeGreaterThanOrEqual(0);
  });
});

describe("bar heights", () => {
  it("makes one bar per band, never below the idle dot, never above the pill", () => {
    const h = barHeights(new Array(BAR_COUNT).fill(0), 20, 2);
    expect(h).toHaveLength(BAR_COUNT);
    expect(Math.min(...h)).toBe(2);
    const full = barHeights(new Array(BAR_COUNT).fill(1), 20, 2);
    expect(Math.max(...full)).toBe(20);
  });

  it("puts the loudest speech bands in the middle", () => {
    // Low bands carry most speech energy; the pill mirrors them towards the centre.
    const levels = Array.from({ length: BAR_COUNT }, (_, i) => (i === 0 ? 1 : 0));
    const h = barHeights(levels, 20, 2);
    const tallest = h.indexOf(Math.max(...h));
    expect(tallest === BAR_COUNT / 2 - 1 || tallest === BAR_COUNT / 2).toBe(true);
  });
});
