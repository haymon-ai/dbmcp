//! UK-specific recognizers.

pub(super) use super::Recognizer;

mod bank_account_uk;
mod driving_licence_uk;
mod nhs_number;
mod nino_uk;
mod passport_uk;
mod postcode_uk;
mod sort_code_uk;
mod vehicle_registration_uk;

pub use bank_account_uk::bank_account_uk;
pub use driving_licence_uk::driving_licence_uk;
pub use nhs_number::nhs_number;
pub use nino_uk::nino_uk;
pub use passport_uk::passport_uk;
pub use postcode_uk::postcode_uk;
pub use sort_code_uk::sort_code_uk;
pub use vehicle_registration_uk::vehicle_registration_uk;
