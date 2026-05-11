//! UK-specific recognizers.

pub(super) use super::Recognizer;

mod bank_account_uk;
mod nhs_number;
mod nino_uk;
mod passport_uk;
mod sort_code_uk;

pub use bank_account_uk::bank_account_uk;
pub use nhs_number::nhs_number;
pub use nino_uk::nino_uk;
pub use passport_uk::passport_uk;
pub use sort_code_uk::sort_code_uk;
