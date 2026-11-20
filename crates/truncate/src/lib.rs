//! Character caps that fit a list of JSON values into a char budget.
//!
//! [`Caps`] holds two caps, both counted in Unicode scalar values:
//!
//! - **per string**: every string value at any depth longer than the cap
//!   is cut and ends in `…[truncated, N chars total]`; object keys are
//!   kept verbatim;
//! - **budget**: the chars the items may take once serialized as one JSON
//!   array, brackets and commas included. Trailing items are dropped once
//!   the array exceeds it. At least one item is always kept, so a caller
//!   that resumes after the kept items always makes progress.
//!
//! A cap of `None` disables it.
//!
//! [`fit`] applies both caps to items already in memory. [`Window`] applies
//! the budget while items arrive: it keeps only the items whose estimated
//! size fits and counts the rest without storing them, so a large input
//! never sits in memory whole. [`fit`] then applies the exact caps to the
//! items the window kept.

mod caps;
mod chars;
mod fit;
mod window;

pub use caps::Caps;
pub use fit::{Fitted, fit};
pub use window::Window;
