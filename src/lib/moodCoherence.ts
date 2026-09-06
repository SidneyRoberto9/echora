import type { MoodTraits } from "./api";

/** One mood in an active mix, with its current slider weight (0-100,
 * matching the units `MoodMixBar` already uses — weights across the mix
 * sum to 100, not 1). */
export interface MoodMixMember {
  id: string;
  name: string;
  traits: MoodTraits;
  weight: number;
}

export type CoherenceLevel = "empty" | "solo" | "harmonious" | "blended" | "tense" | "clashing";

export interface MoodCoherence {
  level: CoherenceLevel;
  message: string;
}

const TRAIT_AXES = ["energy", "darkness", "romance", "sadness", "aggression", "focus"] as const;
type TraitAxis = (typeof TRAIT_AXES)[number];

const AXIS_LABEL: Record<TraitAxis, string> = {
  energy: "Energy",
  darkness: "Darkness",
  romance: "Romance",
  sadness: "Sadness",
  aggression: "Aggression",
  focus: "Focus",
};

const AXIS_DESCRIPTOR: Record<TraitAxis, { low: string; high: string }> = {
  energy: { low: "mellow", high: "high-energy" },
  darkness: { low: "bright", high: "dark" },
  romance: { low: "unromantic", high: "romantic" },
  sadness: { low: "upbeat", high: "somber" },
  aggression: { low: "gentle", high: "aggressive" },
  focus: { low: "loose", high: "focused" },
};

interface PairDivergence {
  score: number;
  axis: TraitAxis;
  higher: MoodMixMember;
  lower: MoodMixMember;
}

/**
 * Divergence between two moods, driven by their single most-different
 * trait axis rather than a Euclidean norm across all six. Two moods can be
 * identical on five axes and far apart on one (e.g. Badass vs Hype differ
 * almost only in `darkness`) — averaging that gap in with five near-zero
 * ones would dilute a real, single-axis incompatibility into "roughly
 * similar". Taking the max keeps it visible and names the axis that
 * actually diverges.
 *
 * The raw axis gap is then dampened by how lopsided this pair's weight
 * split is: a mood mixed at 90/10 is dominated by the majority mood, so a
 * clash with the 10% minority matters far less than the same clash at
 * 50/50. `balance` is 1.0 at an even split and falls toward 0 as one side
 * approaches 0%.
 */
function pairDivergence(a: MoodMixMember, b: MoodMixMember): PairDivergence {
  let axis: TraitAxis = TRAIT_AXES[0];
  let maxDiff = -1;
  for (const ax of TRAIT_AXES) {
    const diff = Math.abs(a.traits[ax] - b.traits[ax]);
    if (diff > maxDiff) {
      maxDiff = diff;
      axis = ax;
    }
  }

  const higher = a.traits[axis] >= b.traits[axis] ? a : b;
  const lower = higher === a ? b : a;

  const totalWeight = a.weight + b.weight;
  const balance = totalWeight > 0 ? 1 - Math.abs(a.weight - b.weight) / totalWeight : 0;

  return { score: maxDiff * balance, axis, higher, lower };
}

/**
 * Pure, heuristic coherence signal for a mood mix — not a measured
 * statistic. Scores every pair of moods in the mix and reports the worst
 * (weight-dampened) clash, since a mix is only as coherent as its most
 * divergent pair.
 */
export function evaluateMoodCoherence(mix: MoodMixMember[]): MoodCoherence {
  if (mix.length === 0) {
    return { level: "empty", message: "No moods selected." };
  }
  if (mix.length === 1) {
    return { level: "solo", message: "Add another mood to see how they blend." };
  }

  let worst = pairDivergence(mix[0], mix[1]);
  for (let i = 0; i < mix.length; i++) {
    for (let j = i + 1; j < mix.length; j++) {
      if (i === 0 && j === 1) continue;
      const candidate = pairDivergence(mix[i], mix[j]);
      if (candidate.score > worst.score) worst = candidate;
    }
  }

  const { score, axis, higher, lower } = worst;
  const label = AXIS_LABEL[axis];
  const descriptor = AXIS_DESCRIPTOR[axis];

  if (score < 10) {
    return { level: "harmonious", message: "This mix reads consistent across every trait." };
  }
  if (score < 35) {
    return {
      level: "blended",
      message: `Mostly aligned, with a moderate ${label.toLowerCase()} gap between ${higher.name} and ${lower.name}.`,
    };
  }
  if (score < 65) {
    return {
      level: "tense",
      message: `${label} pulls this mix apart — ${higher.name} leans ${descriptor.high}, ${lower.name} leans ${descriptor.low}.`,
    };
  }
  return {
    level: "clashing",
    message: `${label} is a hard clash here — ${higher.name} leans ${descriptor.high}, ${lower.name} leans ${descriptor.low}. Still playable, just know what you're getting.`,
  };
}
