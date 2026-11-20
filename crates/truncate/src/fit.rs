//! Exact string and budget caps applied to items already in memory.

use serde_json::Value;

use crate::caps::Caps;
use crate::chars::{CharCount, cut_strings};

/// Items after [`fit`] applied the caps.
#[derive(Debug)]
pub struct Fitted {
    /// Items kept, with long string values cut.
    pub items: Vec<Value>,
    /// Whether the string cap cut a value in a kept item.
    pub cut: bool,
    /// Trailing items the budget dropped.
    pub dropped: u64,
}

/// Cuts long string values and drops trailing items past the budget.
///
/// Dropped items are always trailing, so `items.len()` of the result is
/// the count of leading items kept.
///
/// # Panics
///
/// Never in practice: serializing a JSON value into the in-memory char
/// counter cannot fail.
#[must_use]
pub fn fit(mut items: Vec<Value>, caps: Caps) -> Fitted {
    let mut cut = false;

    // `[` and `]` around the items, plus a `,` before every item but the first.
    let mut total = 2;
    let mut kept = items.len();
    for (index, item) in items.iter_mut().enumerate() {
        // Cut before measuring; the cut counts only if the item is kept.
        let item_cut = caps.string.is_some_and(|max| cut_strings(item, max));
        if let Some(budget) = caps.budget {
            let mut chars = CharCount(0);
            serde_json::to_writer(&mut chars, item).expect("a JSON value serializes into a char counter");
            total += chars.0 + usize::from(index > 0);
            if index > 0 && total > budget {
                kept = index;
                break;
            }
        }
        cut |= item_cut;
    }

    let dropped = (items.len() - kept) as u64;
    items.truncate(kept);
    Fitted { items, cut, dropped }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const fn caps(string: Option<usize>, budget: Option<usize>) -> Caps {
        Caps { string, budget }
    }

    #[test]
    fn short_values_pass_through() {
        let items = vec![json!({"a": "abc", "n": 1})];
        let fitted = fit(items.clone(), caps(Some(3), Some(1000)));
        assert_eq!(fitted.items, items);
        assert!(!fitted.cut);
        assert_eq!(fitted.dropped, 0);
    }

    #[test]
    fn long_value_is_cut_at_a_char_boundary() {
        let fitted = fit(vec![json!({"a": "äöüß€x"})], caps(Some(4), None));
        assert_eq!(fitted.items, vec![json!({"a": "äöüß…[truncated, 6 chars total]"})]);
        assert!(fitted.cut);
    }

    #[test]
    fn nested_leaves_are_cut_and_keys_kept() {
        let long = "x".repeat(10);
        let fitted = fit(vec![json!({"doc": {&long: [long.clone(), 5]}})], caps(Some(2), None));
        assert_eq!(
            fitted.items,
            vec![json!({"doc": {&long: ["xx…[truncated, 10 chars total]", 5]}})]
        );
    }

    #[test]
    fn none_disables_both_caps() {
        let items: Vec<Value> = (0..50).map(|i| json!({"v": "y".repeat(100), "i": i})).collect();
        let fitted = fit(items.clone(), Caps::default());
        assert_eq!(fitted.items, items);
        assert!(!fitted.cut);
        assert_eq!(fitted.dropped, 0);
    }

    #[test]
    fn trailing_items_past_the_budget_are_dropped() {
        // Each item `{"i":N}` is 7 chars; `[` + 2 items + `,` + `]` = 17.
        let items: Vec<Value> = (0..5).map(|i| json!({"i": i})).collect();
        let fitted = fit(items, caps(None, Some(17)));
        assert_eq!(fitted.items, vec![json!({"i": 0}), json!({"i": 1})]);
        assert_eq!(fitted.dropped, 3);
        assert!(!fitted.cut);
    }

    #[test]
    fn one_item_is_kept_even_over_the_budget() {
        let items = vec![json!({"v": "z".repeat(50)}), json!({"v": 1})];
        let fitted = fit(items, caps(None, Some(0)));
        assert_eq!(fitted.items.len(), 1);
        assert_eq!(fitted.dropped, 1);
    }

    #[test]
    fn a_cut_in_a_dropped_item_is_not_reported() {
        let items = vec![json!({"i": 0}), json!({"v": "x".repeat(1000)})];
        let fitted = fit(items, caps(Some(2), Some(10)));
        assert_eq!(fitted.items, vec![json!({"i": 0})]);
        assert_eq!(fitted.dropped, 1);
        assert!(!fitted.cut);
    }
}
