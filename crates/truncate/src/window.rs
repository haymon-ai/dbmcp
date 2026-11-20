//! Budget applied while items arrive, before [`fit`](crate::fit).

use serde_json::Value;

use crate::caps::Caps;
use crate::chars::{CharCount, cut_len};

/// Most item slots [`Window::new`] reserves up front.
const RESERVE: usize = 1024;

/// Items pushed into the budget, plus a count of the items past it.
///
/// Feed every item to [`push`](Window::push). The window skips the first
/// `skip` items, then keeps items while it holds fewer than `take` and
/// their estimated size fits the budget. Once an item is not kept, the
/// window is full: every later item only bumps [`rest`](Window::rest), so
/// kept items stay contiguous.
///
/// Read or take [`items`](Window::items) and [`rest`](Window::rest) only
/// after the last `push`: the window tracks the size of the kept items
/// itself, so changing them in between breaks the budget.
#[derive(Debug)]
pub struct Window {
    /// Leading items still to skip.
    skip: u64,
    /// Most items to keep; `None` = bounded by the budget only.
    take: Option<usize>,
    /// Caps [`fit`](crate::fit) applies later; the estimate uses both.
    caps: Caps,
    /// Items kept, uncut and in arrival order.
    pub items: Vec<Value>,
    /// Items pushed after the window was full; counted, not stored.
    pub rest: u64,
    /// Estimated serialized chars of the kept items, brackets and commas included.
    chars: usize,
}

impl Window {
    /// Creates an empty window skipping `skip` items and keeping up to `take`.
    #[must_use]
    pub fn new(skip: u64, take: Option<usize>, caps: Caps) -> Self {
        Self {
            skip,
            take,
            caps,
            // Reserve at most `RESERVE` slots, so a huge `take` cannot overflow.
            items: Vec::with_capacity(take.unwrap_or(0).min(RESERVE)),
            rest: 0,
            // `[` and `]` around the items.
            chars: 2,
        }
    }

    /// Skips, keeps, or counts the next item.
    ///
    /// The size estimate measures a copy of `item` cut to the string cap,
    /// as [`fit`](crate::fit) will; the kept item stays uncut. The first
    /// kept item is kept whatever its size.
    ///
    /// # Panics
    ///
    /// Never in practice: serializing a JSON value into the in-memory char
    /// counter cannot fail.
    pub fn push(&mut self, item: Value) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        if self.rest > 0 || self.take.is_some_and(|take| self.items.len() >= take) {
            self.rest += 1;
            return;
        }
        if let Some(budget) = self.caps.budget {
            let total = self.chars + self.estimate(&item) + usize::from(!self.items.is_empty());
            if !self.items.is_empty() && total > budget {
                self.rest += 1;
                return;
            }
            self.chars = total;
        }
        self.items.push(item);
    }

    /// Serialized chars of `item` once [`fit`](crate::fit) cuts its long strings.
    ///
    /// Measures `item` in place, so a large value is never copied.
    fn estimate(&self, item: &Value) -> usize {
        let mut chars = CharCount(0);
        if let Some(max) = self.caps.string {
            cut_len(item, max, &mut chars);
        } else {
            serde_json::to_writer(&mut chars, item).expect("a JSON value serializes into a char counter");
        }
        chars.0
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const fn caps(string: Option<usize>, budget: Option<usize>) -> Caps {
        Caps { string, budget }
    }

    /// Pushes `items` into `window` and returns it.
    fn filled(mut window: Window, items: impl IntoIterator<Item = Value>) -> Window {
        for item in items {
            window.push(item);
        }
        window
    }

    /// Items `{"i":0}` .. `{"i":n-1}`, 7 chars each serialized for `n <= 10`.
    fn numbered(n: i64) -> Vec<Value> {
        (0..n).map(|i| json!({"i": i})).collect()
    }

    #[test]
    fn skipped_items_are_not_counted() {
        let window = filled(Window::new(3, Some(2), Caps::default()), numbered(6));
        assert_eq!(window.items, vec![json!({"i": 3}), json!({"i": 4})]);
        assert_eq!(window.rest, 1);

        let window = filled(Window::new(10, None, Caps::default()), numbered(6));
        assert!(window.items.is_empty());
        assert_eq!(window.rest, 0);
    }

    #[test]
    fn keeps_take_items_and_counts_the_rest() {
        let window = filled(Window::new(0, Some(2), caps(None, Some(1000))), numbered(10));
        assert_eq!(window.items, numbered(2));
        assert_eq!(window.rest, 8);

        // Without `take`, the budget alone bounds the items:
        // `[` + 2 items + `,` + `]` = 17.
        let window = filled(Window::new(0, None, caps(None, Some(17))), numbered(10));
        assert_eq!(window.items, numbered(2));
        assert_eq!(window.rest, 8);
    }

    #[test]
    fn stops_at_the_budget_and_keeps_one_item() {
        let window = filled(Window::new(0, None, caps(None, Some(16))), numbered(3));
        assert_eq!(window.items, numbered(1));
        assert_eq!(window.rest, 2);

        let big = json!({"v": "z".repeat(50)});
        let window = filled(
            Window::new(0, None, caps(None, Some(0))),
            [big.clone(), json!({"v": 1})],
        );
        assert_eq!(window.items, vec![big]);
        assert_eq!(window.rest, 1);
    }

    #[test]
    fn estimates_the_cut_item_and_keeps_it_uncut() {
        // Cut to 2 chars, `{"v":"xx…[truncated, 1000 chars total]"}` is 41 chars.
        let huge = json!({"v": "x".repeat(1000)});
        let window = filled(
            Window::new(0, None, caps(Some(2), Some(100))),
            [json!({"i": 0}), huge.clone(), json!({"i": 2})],
        );
        assert_eq!(window.items, vec![json!({"i": 0}), huge, json!({"i": 2})]);
        assert_eq!(window.rest, 0);
    }

    #[test]
    fn no_caps_keep_items_and_measure_uncut() {
        let items: Vec<Value> = (0..50).map(|i| json!({"v": "y".repeat(100), "i": i})).collect();
        let window = filled(Window::new(0, None, Caps::default()), items.clone());
        assert_eq!(window.items, items);
        assert_eq!(window.rest, 0);

        let window = filled(Window::new(0, Some(5), Caps::default()), items.clone());
        assert_eq!(window.items, items[..5]);
        assert_eq!(window.rest, 45);

        // With no string cap, the 1000-char string counts whole and does not fit.
        let huge = json!({"v": "x".repeat(1000)});
        let window = filled(
            Window::new(0, None, caps(None, Some(100))),
            [json!({"i": 0}), huge, json!({"i": 2})],
        );
        assert_eq!(window.items, numbered(1));
        assert_eq!(window.rest, 2);
    }

    #[test]
    fn a_huge_take_does_not_reserve_it_up_front() {
        let window = filled(Window::new(0, Some(usize::MAX), Caps::default()), numbered(3));
        assert_eq!(window.items, numbered(3));
        assert_eq!(window.rest, 0);
    }

    #[test]
    fn stays_full_once_an_item_is_not_kept() {
        let big = json!({"v": "z".repeat(50)});
        let window = filled(
            Window::new(0, None, caps(None, Some(20))),
            [json!({"i": 0}), big, json!({"i": 2})],
        );
        assert_eq!(window.items, numbered(1));
        assert_eq!(window.rest, 2);
    }
}
