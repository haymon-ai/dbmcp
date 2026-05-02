//! Overlap resolution for analyzer and anonymizer results.
//!
//! Algorithm per FR-011 / research §R6:
//!
//! 1. Stable-sort by `(start, -score)`. Stable sort preserves the input order on
//!    ties, which is the registration order the analyzer feeds in.
//! 2. Walk left to right; for each candidate, compare against every kept survivor.
//!    Drop strictly-contained duplicates; on cross-type overlap, higher score wins;
//!    ties broken by longer span, then by registration order (already encoded by
//!    the stable sort).
//! 3. Emit survivors in start-ascending order (the natural order of the walk).

use std::cmp::Ordering;

use crate::result::RecognizerResult;

/// Resolve overlaps in `results`, consuming and returning the surviving spans.
#[must_use]
pub fn resolve(mut results: Vec<RecognizerResult>) -> Vec<RecognizerResult> {
    if results.len() <= 1 {
        return results;
    }
    results.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| score_desc(a, b)));

    let mut survivors: Vec<RecognizerResult> = Vec::with_capacity(results.len());
    for candidate in results {
        let mut keep = true;
        survivors.retain(|existing| {
            if !overlaps(existing, &candidate) {
                return true;
            }
            match dominate(existing, &candidate) {
                Dominance::Existing | Dominance::Equal => {
                    keep = false;
                    true
                }
                Dominance::Candidate => false,
            }
        });
        if keep {
            survivors.push(candidate);
        }
    }
    survivors
}

fn score_desc(a: &RecognizerResult, b: &RecognizerResult) -> Ordering {
    b.score
        .as_f32()
        .partial_cmp(&a.score.as_f32())
        .unwrap_or(Ordering::Equal)
}

fn overlaps(a: &RecognizerResult, b: &RecognizerResult) -> bool {
    a.start < b.end && b.start < a.end
}

enum Dominance {
    Existing,
    Candidate,
    Equal,
}

fn dominate(existing: &RecognizerResult, candidate: &RecognizerResult) -> Dominance {
    // Same entity type and one strictly contains the other → drop contained.
    if existing.entity_type == candidate.entity_type {
        if existing.start <= candidate.start && existing.end >= candidate.end {
            return Dominance::Existing;
        }
        if candidate.start <= existing.start && candidate.end >= existing.end {
            return Dominance::Candidate;
        }
    }
    // Cross-type or partial-overlap same-type: higher score wins; on score tie, longer
    // span wins; on full tie, registration order is already enforced by the stable sort,
    // so the earlier-registered survivor (already in `existing`) wins.
    let by_score = existing
        .score
        .as_f32()
        .partial_cmp(&candidate.score.as_f32())
        .unwrap_or(Ordering::Equal);
    if by_score != Ordering::Equal {
        return if by_score == Ordering::Greater {
            Dominance::Existing
        } else {
            Dominance::Candidate
        };
    }
    match (existing.end - existing.start).cmp(&(candidate.end - candidate.start)) {
        Ordering::Greater => Dominance::Existing,
        Ordering::Less => Dominance::Candidate,
        Ordering::Equal => Dominance::Equal,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::resolve;
    use crate::recognizer::{EntityType, ValidationOutcome};
    use crate::result::{AnalysisExplanation, RecognizerResult};
    use crate::score::Score;

    fn rr(et: &str, start: usize, end: usize, score: f32) -> RecognizerResult {
        let s = Score::new(score).unwrap();
        RecognizerResult {
            entity_type: EntityType::new(et.to_owned()),
            start,
            end,
            score: s,
            explanation: AnalysisExplanation {
                recognizer_name: Cow::Owned(et.to_owned()),
                pattern_name: None,
                original_score: s,
                validation: ValidationOutcome::Unknown,
                final_score: s,
            },
        }
    }

    #[test]
    fn drops_strictly_contained_same_type() {
        let r = resolve(vec![rr("E", 0, 10, 0.5), rr("E", 2, 5, 0.5)]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start, 0);
    }

    #[test]
    fn higher_score_wins_cross_type() {
        let r = resolve(vec![rr("A", 0, 5, 0.3), rr("B", 0, 5, 0.9)]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].entity_type.as_str(), "B");
    }

    #[test]
    fn longer_span_wins_on_score_tie() {
        let r = resolve(vec![rr("A", 0, 8, 0.5), rr("B", 0, 5, 0.5)]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].entity_type.as_str(), "A");
    }

    #[test]
    fn registration_order_breaks_full_tie() {
        // Same start, same end, same score → first registered wins.
        let r = resolve(vec![rr("A", 0, 5, 0.5), rr("B", 0, 5, 0.5)]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].entity_type.as_str(), "A");
    }

    #[test]
    fn non_overlapping_kept() {
        let r = resolve(vec![rr("A", 0, 5, 0.5), rr("B", 10, 15, 0.5)]);
        assert_eq!(r.len(), 2);
    }
}
