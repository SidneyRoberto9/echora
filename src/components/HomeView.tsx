import { useEffect } from "react";
import type { useMoods } from "../hooks/useMoods";
import { MoodCard } from "./MoodCard";
import { SparkleIcon, ChevronRightIcon } from "./icons";
import type { MoodSummary } from "../lib/api";

const CATEGORY_LABEL: Record<string, string> = {
  power: "Power",
  dark: "Dark",
  love: "Love",
  sad: "Sad",
  "energy-lifestyle": "Energy & Lifestyle",
  cinematic: "Cinematic",
};

interface HomeViewProps {
  moodsData: ReturnType<typeof useMoods>;
  onError: (message: string) => void;
  startingMoodId: string | null;
  onStartMood: (moodId: string) => void;
  onSurpriseMe: () => void;
}

export function HomeView({ moodsData, onError, startingMoodId, onStartMood, onSurpriseMe }: HomeViewProps) {
  const { moods, favoriteMoodIds, recentMoodIds, loading, error } = moodsData;

  useEffect(() => {
    if (error) onError(error);
  }, [error, onError]);

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

  const busy = startingMoodId !== null;

  const renderMoodCard = (mood: MoodSummary) => (
    <MoodCard
      key={mood.id}
      mood={mood}
      favorited={favoriteMoodIds.has(mood.id)}
      loading={startingMoodId === mood.id}
      disabled={busy}
      onSelect={onStartMood}
    />
  );

  return (
    <div className="home-view">
      <button
        type="button"
        className="surprise-banner"
        onClick={onSurpriseMe}
        disabled={busy}
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
