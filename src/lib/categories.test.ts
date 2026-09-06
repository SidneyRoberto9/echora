import { describe, expect, it } from "vitest";
import { CATEGORY_COLOR, CATEGORY_LABEL } from "./categories";

const KNOWN_KEYS = ["power", "dark", "love", "sad", "energy-lifestyle", "cinematic"];

describe("CATEGORY_LABEL", () => {
  it("has a human-readable label for every known category", () => {
    expect(CATEGORY_LABEL.power).toBe("Power");
    expect(CATEGORY_LABEL["energy-lifestyle"]).toBe("Energy & Lifestyle");
  });

  it("returns undefined for an unknown category key, matching the `?? category.key` fallback callers rely on", () => {
    expect(CATEGORY_LABEL["not-a-real-category"]).toBeUndefined();
  });
});

describe("CATEGORY_COLOR", () => {
  it("returns undefined for an unknown category key, matching the `?? fallback` callers rely on", () => {
    expect(CATEGORY_COLOR["not-a-real-category"]).toBeUndefined();
  });
});

describe("CATEGORY_LABEL and CATEGORY_COLOR", () => {
  it("define entries for exactly the same set of category keys", () => {
    expect(Object.keys(CATEGORY_LABEL).sort()).toEqual(Object.keys(CATEGORY_COLOR).sort());
  });

  it("cover every category key the app actually renders", () => {
    for (const key of KNOWN_KEYS) {
      expect(CATEGORY_LABEL[key]).toBeDefined();
      expect(CATEGORY_COLOR[key]).toBeDefined();
    }
  });
});
