//! Pan-EU recognizers (custom `EUR` pseudo-code).

pub(super) use super::Recognizer;

mod vat_number;

pub use vat_number::vat_number_eur;
