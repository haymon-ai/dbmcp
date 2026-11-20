//! The two character caps [`fit`](crate::fit) and [`Window`](crate::Window) apply.

/// Character caps for a list of JSON values; `None` disables a cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// Most chars any string value keeps before it is cut.
    pub string: Option<usize>,
    /// Most chars the items take serialized as one JSON array.
    pub budget: Option<usize>,
}
