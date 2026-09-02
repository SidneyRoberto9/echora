use tokio::task::JoinHandle;

use crate::error::{EchoraError, Result};
use crate::models::ResolvedStream;

/// Holds at most one in-flight (or completed) background resolve for the
/// track predicted to play next, so `resolve_and_load` can skip the ~3s
/// yt-dlp/Deno round trip when the prediction was right. Not a general
/// cache: a prefetch for a track that turns out not to be needed is
/// aborted and dropped, never kept around for a later, unrelated play.
#[derive(Default)]
pub struct Prefetch {
    slot: tokio::sync::Mutex<Option<(String, JoinHandle<Result<ResolvedStream>>)>>,
}

impl Prefetch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces any in-flight prefetch with this one, aborting whatever
    /// was there before — it was a prediction for a track that's no
    /// longer next, so finishing it would just waste a yt-dlp call.
    pub async fn spawn(&self, track_id: String, handle: JoinHandle<Result<ResolvedStream>>) {
        let mut slot = self.slot.lock().await;
        if let Some((_, old)) = slot.take() {
            old.abort();
        }
        *slot = Some((track_id, handle));
    }

    /// Takes the prefetch if it was for `track_id`, awaiting it to
    /// completion. Returns `None` (and discards/aborts whatever was
    /// stored) on any mismatch, including nothing having been prefetched.
    pub async fn take_matching(&self, track_id: &str) -> Option<Result<ResolvedStream>> {
        let mut slot = self.slot.lock().await;
        match slot.take() {
            Some((id, handle)) if id == track_id => Some(handle.await.unwrap_or_else(|e| {
                Err(EchoraError::Sidecar(format!("prefetch task failed: {e}")))
            })),
            Some((_, handle)) => {
                handle.abort();
                None
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EchoraError;
    use crate::models::ResolvedStream;
    use std::time::Duration;

    fn resolved(id: &str) -> ResolvedStream {
        ResolvedStream {
            track_id: id.into(),
            stream_url: format!("https://example.invalid/{id}"),
        }
    }

    #[tokio::test]
    async fn take_matching_returns_the_prefetched_result() {
        let prefetch = Prefetch::new();
        let handle = tokio::spawn(async { Ok(resolved("a")) });
        prefetch.spawn("a".into(), handle).await;

        let result = prefetch.take_matching("a").await;

        assert_eq!(result.unwrap().unwrap().track_id, "a");
    }

    #[tokio::test]
    async fn take_matching_with_no_prefetch_returns_none() {
        let prefetch = Prefetch::new();

        assert!(prefetch.take_matching("a").await.is_none());
    }

    #[tokio::test]
    async fn take_matching_with_a_different_track_id_returns_none_and_clears_the_slot() {
        let prefetch = Prefetch::new();
        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(resolved("a"))
        });
        prefetch.spawn("a".into(), handle).await;

        assert!(prefetch.take_matching("b").await.is_none());
        assert!(prefetch.take_matching("a").await.is_none());
    }

    #[tokio::test]
    async fn spawn_replaces_and_aborts_a_stale_in_flight_prefetch() {
        let prefetch = Prefetch::new();
        let stale = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(resolved("a"))
        });
        prefetch.spawn("a".into(), stale).await;

        let fresh = tokio::spawn(async { Ok(resolved("b")) });
        prefetch.spawn("b".into(), fresh).await;

        let result = prefetch.take_matching("b").await;

        assert_eq!(result.unwrap().unwrap().track_id, "b");
    }

    #[tokio::test]
    async fn take_matching_propagates_a_resolve_error() {
        let prefetch = Prefetch::new();
        let handle = tokio::spawn(async { Err(EchoraError::TrackUnavailable("a".into())) });
        prefetch.spawn("a".into(), handle).await;

        let result = prefetch.take_matching("a").await.unwrap();

        assert!(matches!(result, Err(EchoraError::TrackUnavailable(_))));
    }
}
