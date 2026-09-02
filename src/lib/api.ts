import { invoke } from "@tauri-apps/api/core";

export interface Track {
  id: string;
  title: string;
  artist: string | null;
  duration_seconds: number | null;
  thumbnail_url: string | null;
}

export interface MoodTraits {
  energy: number;
  darkness: number;
  romance: number;
  sadness: number;
  aggression: number;
  focus: number;
}

export interface MoodSummary {
  id: string;
  name: string;
  category: string;
  traits: MoodTraits;
}

export interface SessionMood {
  mood_id: string;
  weight: number;
}

export interface SessionInfo {
  id: number;
  moods: SessionMood[];
  started_at: number;
  ended_at: number | null;
}

export interface SessionSummary extends SessionInfo {
  track_count: number;
}

export interface MoodPlayCount {
  mood_id: string;
  play_count: number;
}

export interface CategoryBreakdown {
  category: string;
  session_count: number;
}

export interface ListeningStats {
  total_seconds_listened: number;
  total_sessions: number;
  total_tracks_played: number;
  top_mood_id: string | null;
  category_breakdown: CategoryBreakdown[];
}

export interface SceneSummary {
  id: number;
  name: string;
  created_at: number;
  track_count: number;
}

export interface CrashSummary {
  id: string;
  kind: "Panic" | "SidecarCrash" | "FrontendError";
  timestamp: number;
  message: string;
}

export interface LicenseEntry {
  component: string;
  license: string;
  text: string;
}

export interface QueueView {
  current: Track | null;
  upcoming: Track[];
  /** Absolute index of `current` in the queue — `upcoming[i]` sits at
   * `position + 1 + i`, needed to call `queueSkipTo`/`queueRemove`. */
  position: number | null;
}

export interface Settings {
  cache_limit_mb: number;
  history_enabled: boolean;
  crash_report_enabled: boolean;
  autostart_enabled: boolean;
  sponsorblock_categories: string[];
  volume: number;
}

interface ErrorPayload {
  code: string;
  message: string;
}

/** Thrown for every failed command — `code` is the stable string to switch
 * on, matching `EchoraError::code()` on the Rust side. */
export class ApiError extends Error {
  code: string;
  constructor(payload: ErrorPayload) {
    super(payload.message);
    this.code = payload.code;
  }
}

function isErrorPayload(value: unknown): value is ErrorPayload {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value
  );
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    if (isErrorPayload(err)) throw new ApiError(err);
    throw err;
  }
}

export const api = {
  listMoods: () => call<MoodSummary[]>("list_moods"),
  surpriseMe: () => call<SessionInfo>("surprise_me"),
  isAppimageBuild: () => call<boolean>("is_appimage_build"),
  getThirdPartyLicenses: () => call<LicenseEntry[]>("get_third_party_licenses"),

  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) => call<void>("update_settings", { settings }),

  startMoodSession: (moodId: string) => call<SessionInfo>("start_mood_session", { moodId }),
  startMixedSession: (moods: SessionMood[]) => call<SessionInfo>("start_mixed_session", { moods }),
  endSession: () => call<void>("end_session"),
  getCurrentSession: () => call<SessionInfo | null>("get_current_session"),
  listHistory: (limit: number, offset: number) =>
    call<SessionSummary[]>("list_history", { limit, offset }),
  clearHistory: () => call<void>("clear_history"),
  listCrashReports: () => call<CrashSummary[]>("list_crash_reports"),
  getCrashReportMarkdown: (id: string) => call<string>("get_crash_report_markdown", { id }),
  clearCrashReports: () => call<void>("clear_crash_reports"),
  reportFrontendCrash: (message: string, stack?: string) =>
    call<void>("report_frontend_crash", { message, stack: stack ?? null }),

  getQueue: () => call<QueueView>("get_queue"),
  queueNext: () => call<Track | null>("queue_next"),
  queuePrevious: () => call<Track | null>("queue_previous"),
  queueSkipTo: (index: number) => call<Track>("queue_skip_to", { index }),
  queueRemove: (index: number) => call<void>("queue_remove", { index }),
  ensureQueueToppedUp: () => call<void>("ensure_queue_topped_up"),

  favoriteTrack: (track: Track) => call<void>("favorite_track", { track }),
  unfavoriteTrack: (trackId: string) => call<void>("unfavorite_track", { trackId }),
  isTrackFavorited: (trackId: string) => call<boolean>("is_track_favorited", { trackId }),
  favoriteMood: (moodId: string) => call<void>("favorite_mood", { moodId }),
  unfavoriteMood: (moodId: string) => call<void>("unfavorite_mood", { moodId }),
  listFavoriteMoods: () => call<string[]>("list_favorite_moods"),
  setTrackFeedback: (track: Track, liked: boolean) =>
    call<void>("set_track_feedback", { track, liked }),
  getTrackFeedback: (trackId: string) => call<boolean | null>("get_track_feedback", { trackId }),

  listFavoriteTracks: () => call<Track[]>("list_favorite_tracks"),
  listMostPlayedMoods: () => call<MoodPlayCount[]>("list_most_played_moods"),
  getListeningStats: () => call<ListeningStats>("get_listening_stats"),
  playSingleTrack: (track: Track) => call<void>("play_single_track", { track }),
  saveScene: (name: string) => call<SceneSummary>("save_scene", { name }),
  listScenes: () => call<SceneSummary[]>("list_scenes"),
  playScene: (sceneId: number) => call<void>("play_scene", { sceneId }),
  renameScene: (sceneId: number, name: string) =>
    call<void>("rename_scene", { sceneId, name }),
  deleteScene: (sceneId: number) => call<void>("delete_scene", { sceneId }),

  pausePlayback: () => call<void>("pause_playback"),
  resumePlayback: () => call<void>("resume_playback"),
  seekPlayback: (seconds: number) => call<void>("seek_playback", { seconds }),
  setPlaybackVolume: (volume: number) => call<void>("set_playback_volume", { volume }),
  getPlaybackPosition: () => call<number | null>("get_playback_position"),
  getPlaybackDuration: () => call<number | null>("get_playback_duration"),
};
