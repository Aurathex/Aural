import { describe, expect, it } from "vitest";
import { formatBytes, percent } from "./format";

describe("formatBytes", () => {
  it("uses MB below a gigabyte and GB above, with sensible precision", () => {
    expect(formatBytes(81_781_811)).toBe("82 MB");
    expect(formatBytes(661_331_448)).toBe("661 MB");
    expect(formatBytes(2_435_420_160)).toBe("2.4 GB");
    expect(formatBytes(512)).toBe("1 MB");
  });
});

describe("percent", () => {
  it("is an integer 0..100 and safe for zero totals", () => {
    expect(percent(50, 200)).toBe(25);
    expect(percent(1, 3)).toBe(33);
    expect(percent(5, 0)).toBe(0);
    expect(percent(300, 200)).toBe(100);
  });
});
