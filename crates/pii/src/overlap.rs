//! Overlap resolution for analyzer and anonymizer results.
//!
//! No part of a detected span is left uncovered. Rules, in order:
//!
//! 1. Overlapping spans of one entity type merge into their union, with the
//!    highest score (and that part's explanation) and `Pattern` if any part
//!    came from a pattern recognizer.
//! 2. A span strictly inside another is dropped; the container wins whatever
//!    its type, score, or detector.
//! 3. The rest claim bytes strongest first. Strength: [`Detector::Pattern`]
//!    before [`Detector::Ner`] (scores from different detectors are not
//!    comparable), then higher score, then longer span, then earlier input
//!    position (registration order). A span overlapped by a stronger one keeps
//!    only its unclaimed bytes, whitespace-stripped at both ends, and is dropped
//!    if nothing is left; equal extents thus go to the stronger span.
//! 4. Survivors are emitted in start-ascending order.
//!
//! Touching spans (`a.end == b.start`) do not overlap.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::result::{Detector, RecognizerResult};

/// Resolves overlaps in `results` into disjoint spans over `text`.
///
/// Every non-whitespace byte covered by an input span is covered by an output
/// span. Output spans are disjoint, sorted by start, on char boundaries, and
/// each byte lies inside an input span of the same type. Resolving the output
/// again returns it unchanged.
#[must_use]
pub fn resolve(text: &str, mut results: Vec<RecognizerResult>) -> Vec<RecognizerResult> {
    if results.len() <= 1 {
        return results;
    }
    // Disjoint input (the common multi-span case) is already resolved.
    let mut extents: Vec<(usize, usize)> = results.iter().map(|r| (r.start, r.end)).collect();
    extents.sort_unstable();
    if extents.windows(2).all(|w| w[0].1 <= w[1].0) {
        results.sort_by_key(|r| r.start);
        return results;
    }
    let mut ranked: Vec<(usize, RecognizerResult)> = drop_contained(merge_same_type(results));
    ranked.sort_by(|(ia, a), (ib, b)| strength(a, b).then(ia.cmp(ib)));

    // Claimed byte ranges, disjoint, keyed by start.
    let mut claimed: BTreeMap<usize, usize> = BTreeMap::new();
    let mut survivors: Vec<RecognizerResult> = Vec::with_capacity(ranked.len());
    for (_, result) in ranked {
        let pieces = unclaimed(&claimed, result.start, result.end);
        if pieces == [(result.start, result.end)] {
            claimed.insert(result.start, result.end);
            survivors.push(result);
            continue;
        }
        for (start, end) in pieces {
            let Some((start, end)) = strip(text, start, end) else {
                continue;
            };
            claimed.insert(start, end);
            survivors.push(RecognizerResult {
                start,
                end,
                ..result.clone()
            });
        }
    }
    survivors.sort_by_key(|r| r.start);
    survivors
}

/// Folds overlapping spans of one entity type into their union, tagged by input position.
///
/// The union keeps the earliest input position, the highest score with that
/// part's explanation (tie: earlier input), and `Pattern` if any part has it.
fn merge_same_type(results: Vec<RecognizerResult>) -> Vec<(usize, RecognizerResult)> {
    let mut spans: Vec<(usize, RecognizerResult)> = results.into_iter().enumerate().collect();
    spans.sort_by_key(|(_, r)| (r.entity_type, r.start, r.end));
    let mut out: Vec<(usize, RecognizerResult)> = Vec::with_capacity(spans.len());
    for (i, span) in spans {
        let Some((at, merged)) = out
            .last_mut()
            .filter(|(_, m)| m.entity_type == span.entity_type && span.start < m.end)
        else {
            out.push((i, span));
            continue;
        };
        let higher = span.score > merged.score || (span.score == merged.score && i < *at);
        merged.end = merged.end.max(span.end);
        if span.detector == Detector::Pattern {
            merged.detector = Detector::Pattern;
        }
        if higher {
            merged.score = span.score;
            merged.explanation = span.explanation;
        }
        *at = (*at).min(i);
    }
    out
}

/// Drops every span strictly contained in another, keeping input positions.
fn drop_contained(mut spans: Vec<(usize, RecognizerResult)>) -> Vec<(usize, RecognizerResult)> {
    spans.sort_by(|(_, a), (_, b)| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut out: Vec<(usize, RecognizerResult)> = Vec::with_capacity(spans.len());
    // Furthest end so far. Every earlier span starts no later, so reaching
    // `end` with a different extent means strict containment; a span with the
    // extent of the last kept one is kept with it.
    let mut reach = 0;
    for (i, span) in spans {
        let same = out
            .last()
            .is_some_and(|(_, last)| (last.start, last.end) == (span.start, span.end));
        let contained = !same && reach >= span.end;
        reach = reach.max(span.end);
        if !contained {
            out.push((i, span));
        }
    }
    out
}

/// Orders spans strongest first: Pattern, then score, then length.
fn strength(a: &RecognizerResult, b: &RecognizerResult) -> Ordering {
    (b.detector == Detector::Pattern)
        .cmp(&(a.detector == Detector::Pattern))
        .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal))
        .then_with(|| (b.end - b.start).cmp(&(a.end - a.start)))
}

/// Returns the sub-ranges of `start..end` outside every claimed range.
fn unclaimed(claimed: &BTreeMap<usize, usize>, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut hits: Vec<(usize, usize)> = claimed
        .range(..end)
        .rev()
        .map(|(&s, &e)| (s, e))
        .take_while(|&(_, e)| e > start)
        .collect();
    hits.reverse();
    let mut pieces = Vec::new();
    let mut cursor = start;
    for (s, e) in hits {
        if s > cursor {
            pieces.push((cursor, s));
        }
        cursor = cursor.max(e);
    }
    if cursor < end {
        pieces.push((cursor, end));
    }
    pieces
}

/// Strips whitespace from both ends of `start..end`; `None` if nothing is left.
fn strip(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = text.get(start..end)?;
    let lead = slice.len() - slice.trim_start().len();
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some((start + lead, start + lead + trimmed.len()))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::resolve;
    use crate::result::{AnalysisExplanation, Detector, RecognizerResult};
    use crate::score::Score;
    use crate::{Entity, ValidationOutcome};

    fn spans(r: &[RecognizerResult]) -> Vec<(Entity, usize, usize)> {
        r.iter().map(|x| (x.entity_type, x.start, x.end)).collect()
    }

    const TEXT: &str = "abcdefghijklmnopqrstuvwxyz";

    fn rr(et: Entity, start: usize, end: usize, score: f32, detector: Detector) -> RecognizerResult {
        let s = Score::new(score).unwrap();
        RecognizerResult {
            entity_type: et,
            start,
            end,
            score: s,
            explanation: AnalysisExplanation {
                recognizer_name: Cow::Borrowed(et.as_str()),
                pattern_name: None,
                original_score: s,
                validation: ValidationOutcome::Unknown,
                final_score: s,
                supportive_keyword: None,
            },
            detector,
        }
    }

    #[test]
    fn drops_strictly_contained_same_type() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 10, 0.5, Detector::Pattern),
                rr(Entity::EmailAddress, 2, 5, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 10)]);
    }

    #[test]
    fn higher_score_wins_cross_type() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 5, 0.3, Detector::Pattern),
                rr(Entity::CreditCard, 0, 5, 0.9, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::CreditCard, 0, 5)]);
    }

    #[test]
    fn cross_type_container_wins_over_contained() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 8, 0.5, Detector::Pattern),
                rr(Entity::CreditCard, 0, 5, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 8)]);
    }

    #[test]
    fn registration_order_breaks_full_tie() {
        // Same start, same end, same score → first registered wins.
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 5, 0.5, Detector::Pattern),
                rr(Entity::CreditCard, 0, 5, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 5)]);
    }

    #[test]
    fn non_overlapping_kept() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 5, 0.5, Detector::Pattern),
                rr(Entity::CreditCard, 10, 15, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 5), (Entity::CreditCard, 10, 15)]);
    }

    #[test]
    fn container_wins_over_higher_scoring_ner_inside() {
        let text = "anna.schmidt@company.example";
        let r = resolve(
            text,
            vec![
                rr(Entity::EmailAddress, 0, 28, 0.5, Detector::Pattern),
                rr(Entity::Person, 0, 12, 0.99, Detector::Ner),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 28)]);
    }

    #[test]
    fn pattern_beats_higher_scoring_ner_on_equal_extents() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::Person, 0, 5, 0.99, Detector::Ner),
                rr(Entity::CreditCard, 0, 5, 0.4, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::CreditCard, 0, 5)]);
    }

    #[test]
    fn partial_ner_trimmed_outside_pattern() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::Person, 0, 5, 0.99, Detector::Ner),
                rr(Entity::CreditCard, 3, 8, 0.4, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::Person, 0, 3), (Entity::CreditCard, 3, 8)]);
    }

    #[test]
    fn same_type_partial_merges_into_union() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::Person, 0, 5, 0.5, Detector::Ner),
                rr(Entity::Person, 3, 8, 0.9, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::Person, 0, 8)]);
        assert_eq!(r[0].score, Score::from_static(0.9));
        assert_eq!(r[0].detector, Detector::Pattern);
        assert_eq!(r[0].explanation.final_score, Score::from_static(0.9));
    }

    #[test]
    fn ner_container_wins_over_pattern_inside() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::Person, 0, 10, 0.4, Detector::Ner),
                rr(Entity::EmailAddress, 2, 6, 1.0, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::Person, 0, 10)]);
    }

    #[test]
    fn same_detector_partial_higher_score_whole_other_trimmed() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 5, 0.9, Detector::Pattern),
                rr(Entity::CreditCard, 3, 10, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 5), (Entity::CreditCard, 5, 10)]);
    }

    #[test]
    fn same_detector_partial_score_tie_longer_whole() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::EmailAddress, 0, 5, 0.5, Detector::Ner),
                rr(Entity::CreditCard, 3, 10, 0.5, Detector::Ner),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 3), (Entity::CreditCard, 3, 10)]);
    }

    #[test]
    fn same_detector_partial_full_tie_earlier_input_whole() {
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::CreditCard, 3, 8, 0.5, Detector::Pattern),
                rr(Entity::EmailAddress, 0, 5, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 0, 3), (Entity::CreditCard, 3, 8)]);
    }

    #[test]
    fn trimmed_remainder_strips_whitespace() {
        let text = "Anna Schmidt anna@company.example";
        let r = resolve(
            text,
            vec![
                rr(Entity::Person, 0, 17, 0.99, Detector::Ner),
                rr(Entity::EmailAddress, 13, 33, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::Person, 0, 12), (Entity::EmailAddress, 13, 33)]);
    }

    #[test]
    fn whitespace_only_remainder_dropped() {
        let text = "ab   cdefgh";
        let r = resolve(
            text,
            vec![
                rr(Entity::Person, 2, 6, 0.99, Detector::Ner),
                rr(Entity::EmailAddress, 5, 11, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 5, 11)]);
    }

    #[test]
    fn untrimmed_span_keeps_its_whitespace() {
        let text = "ab cd ef";
        let r = resolve(
            text,
            vec![
                rr(Entity::EmailAddress, 2, 6, 0.9, Detector::Pattern),
                rr(Entity::CreditCard, 5, 8, 0.5, Detector::Pattern),
            ],
        );
        assert_eq!(spans(&r), [(Entity::EmailAddress, 2, 6), (Entity::CreditCard, 6, 8)]);
    }

    #[test]
    fn chain_of_partial_overlaps() {
        // A 0..6 (strongest), B 4..12, C 10..16: B keeps the middle, C the tail.
        let r = resolve(
            TEXT,
            vec![
                rr(Entity::CreditCard, 0, 6, 0.9, Detector::Pattern),
                rr(Entity::EmailAddress, 4, 12, 0.7, Detector::Pattern),
                rr(Entity::IbanCode, 10, 16, 0.8, Detector::Pattern),
            ],
        );
        assert_eq!(
            spans(&r),
            [
                (Entity::CreditCard, 0, 6),
                (Entity::EmailAddress, 6, 10),
                (Entity::IbanCode, 10, 16)
            ]
        );
    }

    /// Checks the coverage, disjointness, provenance and idempotence invariants.
    fn assert_invariants(text: &str, input: &[RecognizerResult]) {
        let out = resolve(text, input.to_vec());
        for (b, ch) in text.bytes().enumerate() {
            let covered_in = input.iter().any(|r| r.start <= b && b < r.end);
            let covered_out = out.iter().any(|r| r.start <= b && b < r.end);
            assert!(
                !covered_in || ch.is_ascii_whitespace() || covered_out,
                "byte {b} leaked: {:?} -> {:?}",
                spans(input),
                spans(&out)
            );
        }
        for pair in out.windows(2) {
            assert!(
                pair[0].end <= pair[1].start,
                "overlap: {:?} -> {:?}",
                spans(input),
                spans(&out)
            );
        }
        for r in &out {
            assert!(r.start < r.end, "empty span: {:?} -> {:?}", spans(input), spans(&out));
            for b in r.start..r.end {
                assert!(
                    input
                        .iter()
                        .any(|i| i.entity_type == r.entity_type && i.start <= b && b < i.end),
                    "byte {b} of {:?} has no same-type input: {:?}",
                    (r.entity_type, r.start, r.end),
                    spans(input)
                );
            }
        }
        assert_eq!(resolve(text, out.clone()), out, "not idempotent: {:?}", spans(input));
    }

    /// All spans `start..end` with both bounds in `bounds`.
    fn extents(bounds: &[usize]) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (i, &s) in bounds.iter().enumerate() {
            for &e in &bounds[i + 1..] {
                v.push((s, e));
            }
        }
        v
    }

    #[test]
    fn coverage_invariant_holds_for_every_layout() {
        let text = "ab cd ef gh ij";
        // Types × (detector, score): same/different type and detector, lower/equal/higher score.
        let mut kinds = Vec::new();
        for et in [Entity::EmailAddress, Entity::CreditCard, Entity::Person] {
            for (d, sc) in [(Detector::Pattern, 0.5), (Detector::Ner, 0.5), (Detector::Ner, 0.9)] {
                kinds.push((et, d, sc));
            }
        }
        let make = |(s, e): (usize, usize), (et, d, sc): (Entity, Detector, f32)| rr(et, s, e, sc, d);

        // Pairs: every extent over the first nine bytes covers all two-span layouts.
        let all: Vec<usize> = (0..=9).collect();
        let pair_extents = extents(&all);
        for &a in &pair_extents {
            for &b in &pair_extents {
                for &ka in &kinds {
                    for &kb in &kinds {
                        assert_invariants(text, &[make(a, ka), make(b, kb)]);
                    }
                }
            }
        }

        // Triples: coarser bounds and two types keep the loop small but hit every layout.
        let triple_extents = extents(&[0, 2, 3, 5, 8]);
        let triple_kinds: Vec<_> = kinds.iter().copied().filter(|k| k.0 != Entity::CreditCard).collect();
        for &a in &triple_extents {
            for &b in &triple_extents {
                for &c in &triple_extents {
                    for &ka in &triple_kinds {
                        for &kb in &triple_kinds {
                            for &kc in &triple_kinds {
                                assert_invariants(text, &[make(a, ka), make(b, kb), make(c, kc)]);
                            }
                        }
                    }
                }
            }
        }
    }
}
