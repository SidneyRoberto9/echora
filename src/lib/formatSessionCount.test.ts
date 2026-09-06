import { describe, expect, it } from "vitest";
import { formatSessionCount } from "./formatSessionCount";

describe("formatSessionCount", () => {
  it("returns 0 for zero sessions", () => {
    expect(formatSessionCount(0)).toBe(0);
  });

  it("passes integers through unchanged", () => {
    expect(formatSessionCount(1)).toBe(1);
    expect(formatSessionCount(42)).toBe(42);
  });

  it("rounds fractional averages down below the half", () => {
    expect(formatSessionCount(2.4)).toBe(2);
  });

  it("rounds fractional averages up at and above the half", () => {
    expect(formatSessionCount(2.5)).toBe(3);
    expect(formatSessionCount(2.6)).toBe(3);
  });

  it("rounds negative values toward +Infinity at the half, matching Math.round (not 'round half away from zero')", () => {
    expect(formatSessionCount(-2.5)).toBe(-2);
    expect(formatSessionCount(-2.6)).toBe(-3);
  });
});
