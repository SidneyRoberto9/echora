import { describe, expect, it } from "vitest";
import { evaluateMoodCoherence, type MoodMixMember } from "./moodCoherence";

// Real trait values from src-tauri/resources/moods.json — not invented.
const BADASS: MoodMixMember["traits"] = {
  energy: 85,
  darkness: 65,
  romance: 5,
  sadness: 10,
  aggression: 80,
  focus: 40,
};
const PEACEFUL: MoodMixMember["traits"] = {
  energy: 25,
  darkness: 5,
  romance: 25,
  sadness: 15,
  aggression: 0,
  focus: 50,
};
const IN_LOVE: MoodMixMember["traits"] = {
  energy: 70,
  darkness: 10,
  romance: 90,
  sadness: 10,
  aggression: 5,
  focus: 40,
};
const ROMANTIC: MoodMixMember["traits"] = {
  energy: 65,
  darkness: 10,
  romance: 90,
  sadness: 15,
  aggression: 5,
  focus: 45,
};
// Nearly identical to Badass on every axis except darkness (65 vs 10) —
// the single-axis-clash case the Euclidean-distance pitfall would flatten.
const HYPE: MoodMixMember["traits"] = {
  energy: 95,
  darkness: 10,
  romance: 5,
  sadness: 5,
  aggression: 80,
  focus: 40,
};

function member(id: string, name: string, traits: MoodMixMember["traits"], weight: number): MoodMixMember {
  return { id, name, traits, weight };
}

describe("evaluateMoodCoherence", () => {
  it("reads a clearly coherent pair (In Love + Romantic) as harmonious", () => {
    const result = evaluateMoodCoherence([
      member("in-love", "In Love", IN_LOVE, 50),
      member("romantic", "Romantic", ROMANTIC, 50),
    ]);
    expect(result.level).toBe("harmonious");
  });

  it("reads a clearly dissonant pair (Badass + Peaceful) as tense/clashing and names the dominant axis", () => {
    const result = evaluateMoodCoherence([
      member("badass", "Badass", BADASS, 50),
      member("peaceful", "Peaceful", PEACEFUL, 50),
    ]);
    expect(result.level === "tense" || result.level === "clashing").toBe(true);
    // aggression has the largest raw gap (80 vs 0 = 80), ahead of energy
    // and darkness (60 each) — the message must name it, not a diluted axis.
    expect(result.message).toContain("Aggression");
  });

  it("does not flatten a single-axis clash into 'similar' via averaging", () => {
    // Badass vs Hype: identical-ish on 5 axes (diffs of 10, 0, 5, 0, 0),
    // 55 points apart on darkness alone. A mean/Euclidean blend across all
    // six axes would read this as mild; the dominant-axis metric must not.
    const result = evaluateMoodCoherence([
      member("badass", "Badass", BADASS, 50),
      member("hype", "Hype", HYPE, 50),
    ]);
    expect(result.level).not.toBe("harmonious");
    expect(result.message).toContain("Darkness");
  });

  it("makes an unbalanced weight (90/10) read less dissonant than the same pair at 50/50", () => {
    const even = evaluateMoodCoherence([
      member("badass", "Badass", BADASS, 50),
      member("peaceful", "Peaceful", PEACEFUL, 50),
    ]);
    const skewed = evaluateMoodCoherence([
      member("badass", "Badass", BADASS, 90),
      member("peaceful", "Peaceful", PEACEFUL, 10),
    ]);
    const levelRank: Record<string, number> = {
      harmonious: 0,
      blended: 1,
      tense: 2,
      clashing: 3,
    };
    expect(levelRank[skewed.level]).toBeLessThan(levelRank[even.level]);
  });

  it("returns a 'solo' signal for a single mood (no mix to evaluate)", () => {
    const result = evaluateMoodCoherence([member("badass", "Badass", BADASS, 100)]);
    expect(result.level).toBe("solo");
  });

  it("returns an 'empty' signal for an empty mix", () => {
    const result = evaluateMoodCoherence([]);
    expect(result.level).toBe("empty");
  });
});
