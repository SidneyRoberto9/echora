import { useEffect, useState } from "react";
import type { useMoods } from "../hooks/useMoods";
import { MoodCard } from "./MoodCard";
import { MoodMixBar } from "./MoodMixBar";
import { SparkleIcon, ChevronRightIcon } from "./icons";
import { CATEGORY_LABEL } from "../lib/categories";
import type { MoodSummary, SessionMood } from "../lib/api";

interface HomeViewProps {
  moodsData: ReturnType<typeof useMoods>;
  onError: (message: string) => void;
  startingMoodId: string | null;
  onStartMood: (moodId: string) => void;
  onStartMix: (moods: SessionMood[]) => void | Promise<void>;
  onSurpriseMe: () => void;
}

function evenWeights(count: number): number[] {
  const base = Math.floor(100 / count);
  const weights = new Array(count).fill(base);
  weights[count - 1] = 100 - base * (count - 1);
  return weights;
}

export function HomeView({
  moodsData,
  onError,
  startingMoodId,
  onStartMood,
  onStartMix,
  onSurpriseMe,
}: HomeViewProps) {
  const { moods, favoriteMoodIds, recentMoodIds, loading, error } = moodsData;
  const [mixMode, setMixMode] = useState(false);
  const [selectedMoodIds, setSelectedMoodIds] = useState<string[]>([]);
  const [weights, setWeights] = useState<number[]>([]);

  useEffect(() => {
    if (error) onError(error);
  }, [error, onError]);

  const busy = startingMoodId !== null;

  const forYouIds = [...favoriteMoodIds, ...recentMoodIds];
  const moodsById = new Map(moods.map((m) => [m.id, m]));
  const forYouMoods = forYouIds.map((id) => moodsById.get(id)).filter((m): m is MoodSummary => !!m);

  const categories: { key: string; moods: MoodSummary[] }[] = [];
  for (const mood of moods) {
    let bucket = categories.find((c) => c.key === mood.category);
    if (!bucket) {
      bucket = { key: mood.category, moods: [] };
      categories.push(bucket);
    }
    bucket.moods.push(mood);
  }

  const toggleMoodSelection = (moodId: string) => {
    setSelectedMoodIds((current) => {
      if (current.includes(moodId)) {
        const next = current.filter((id) => id !== moodId);
        setWeights(evenWeights(Math.max(next.length, 1)));
        return next;
      }
      if (current.length >= 3) return current;
      const next = [...current, moodId];
      setWeights(evenWeights(next.length));
      return next;
    });
  };

  const exitMixMode = () => {
    setMixMode(false);
    setSelectedMoodIds([]);
    setWeights([]);
  };

  const handleStartMix = async () => {
    await onStartMix(selectedMoodIds.map((id, i) => ({ mood_id: id, weight: weights[i] })));
    exitMixMode();
  };

  const selectedMoods = selectedMoodIds
    .map((id) => moodsById.get(id))
    .filter((m): m is MoodSummary => !!m);

  const renderMoodCard = (mood: MoodSummary) => (
    <MoodCard
      key={mood.id}
      mood={mood}
      favorited={favoriteMoodIds.has(mood.id)}
      loading={startingMoodId === mood.id}
      disabled={busy}
      selected={mixMode && selectedMoodIds.includes(mood.id)}
      onSelect={mixMode ? toggleMoodSelection : onStartMood}
    />
  );

  return (
    <div className="home-view">
      <button
        type="button"
        className="surprise-banner"
        onClick={onSurpriseMe}
        disabled={busy || mixMode}
        aria-label="Surprise me — let Echora pick your mood"
      >
        <span className="surprise-banner__label">
          <SparkleIcon />
          <span>
            <div className="surprise-banner__title">
              {busy && startingMoodId === "surprise" ? "Picking…" : "Surprise Me"}
            </div>
            <div className="surprise-banner__subtitle">Let Echora pick your mood</div>
          </span>
        </span>
        <ChevronRightIcon />
      </button>

      <button
        type="button"
        className={`mix-toggle${mixMode ? " is-active" : ""}`}
        onClick={() => (mixMode ? exitMixMode() : setMixMode(true))}
        disabled={busy}
      >
        {mixMode ? "Cancel mix" : "Mix moods"}
      </button>

      {mixMode && selectedMoods.length >= 2 ? (
        <MoodMixBar
          moods={selectedMoods}
          weights={weights}
          onChangeWeights={setWeights}
          onStart={handleStartMix}
          busy={busy}
        />
      ) : null}

      {loading ? (
        <div className="mood-row">
          <div className="mood-row__scroll">
            <div className="skeleton" style={{ width: 148, height: 88, borderRadius: 16 }} />
            <div className="skeleton" style={{ width: 148, height: 88, borderRadius: 16 }} />
            <div className="skeleton" style={{ width: 148, height: 88, borderRadius: 16 }} />
          </div>
        </div>
      ) : (
        <>
          {forYouMoods.length > 0 ? (
            <div className="mood-row">
              <h2 className="mood-row__title">For You</h2>
              <div className="mood-row__scroll">{forYouMoods.map(renderMoodCard)}</div>
            </div>
          ) : null}

          {categories.map((category) => (
            <div className="mood-row" key={category.key}>
              <h2 className="mood-row__title">{CATEGORY_LABEL[category.key] ?? category.key}</h2>
              <div className="mood-row__scroll">{category.moods.map(renderMoodCard)}</div>
            </div>
          ))}
        </>
      )}
    </div>
  );
}
