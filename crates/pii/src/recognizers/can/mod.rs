//! Canada-specific recognizers (ISO 3166-1 alpha-3 `CAN`).

pub(super) use super::Recognizer;

mod sin;

pub use sin::sin_can;
