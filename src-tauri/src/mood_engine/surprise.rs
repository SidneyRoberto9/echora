use std::collections::HashSet;

use rand::Rng;
use rand::distr::weighted::WeightedIndex;
use rand::prelude::Distribution;

use crate::error::{EchoraError, Result};
use crate::models::MoodSummary;

/// Weighted pick for "Surprise Me": favorited moods are more likely, moods
/// played very recently are less likely (for variety), everything else is
/// baseline — simple enough to explain, not a black box (see the product
/// brief's "avoid unpredictable behavior nobody can understand").
pub fn pick_surprise_mood<'a>(
    moods: &'a [MoodSummary],
    favorited: &HashSet<String>,
    recently_played: &HashSet<String>,
    rng: &mut impl Rng,
) -> Result<&'a MoodSummary> {
    if moods.is_empty() {
        return Err(EchoraError::Metadata(
            "no moods available to surprise with".into(),
        ));
    }

    let weights: Vec<f64> = moods
        .iter()
        .map(|mood| {
            let mut weight = 1.0;
            if favorited.contains(&mood.id) {
                weight *= 3.0;
            }
            if recently_played.contains(&mood.id) {
                weight *= 0.3;
            }
            weight
        })
        .collect();

    let distribution =
        WeightedIndex::new(&weights).map_err(|e| EchoraError::Metadata(e.to_string()))?;
    Ok(&moods[distribution.sample(rng)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn moods(ids: &[&str]) -> Vec<MoodSummary> {
        ids.iter()
            .map(|id| MoodSummary {
                id: id.to_string(),
                name: id.to_string(),
                category: "test".into(),
                traits: Default::default(),
            })
            .collect()
    }

    #[test]
    fn errors_on_an_empty_mood_list() {
        let mut rng = StdRng::seed_from_u64(1);
        let err = pick_surprise_mood(&[], &HashSet::new(), &HashSet::new(), &mut rng).unwrap_err();
        assert!(matches!(err, EchoraError::Metadata(_)));
    }

    #[test]
    fn a_single_mood_is_always_picked() {
        let list = moods(&["only"]);
        let mut rng = StdRng::seed_from_u64(1);
        let picked = pick_surprise_mood(&list, &HashSet::new(), &HashSet::new(), &mut rng).unwrap();
        assert_eq!(picked.id, "only");
    }

    #[test]
    fn favoriting_a_mood_makes_it_win_far_more_often() {
        let list = moods(&["villain", "focus", "chill"]);
        let mut favorited = HashSet::new();
        favorited.insert("villain".to_string());
        let mut rng = StdRng::seed_from_u64(99);

        let mut villain_wins = 0;
        for _ in 0..200 {
            let picked = pick_surprise_mood(&list, &favorited, &HashSet::new(), &mut rng).unwrap();
            if picked.id == "villain" {
                villain_wins += 1;
            }
        }
        // Uniform would be ~66/200; favorited (3x weight) should land well above that.
        assert!(
            villain_wins > 90,
            "expected favorited mood to win clearly more often, got {villain_wins}/200"
        );
    }

    #[test]
    fn a_recently_played_mood_wins_far_less_often() {
        let list = moods(&["villain", "focus", "chill"]);
        let mut recently_played = HashSet::new();
        recently_played.insert("villain".to_string());
        let mut rng = StdRng::seed_from_u64(99);

        let mut villain_wins = 0;
        for _ in 0..200 {
            let picked =
                pick_surprise_mood(&list, &HashSet::new(), &recently_played, &mut rng).unwrap();
            if picked.id == "villain" {
                villain_wins += 1;
            }
        }
        // Uniform would be ~66/200; recently-played (0.3x weight) should land well below that.
        assert!(
            villain_wins < 40,
            "expected recently played mood to win clearly less often, got {villain_wins}/200"
        );
    }
}
