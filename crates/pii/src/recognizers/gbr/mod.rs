//! United Kingdom-specific recognizers (ISO 3166-1 alpha-3 `GBR`).

mod bank_account;
mod driving_licence;
mod nhs_number;
mod nino;
mod passport;
mod postcode;
mod sort_code;
mod vehicle_registration;

pub use bank_account::bank_account_gbr;
pub use driving_licence::driving_licence_gbr;
pub use nhs_number::nhs_number_gbr;
pub use nino::nino_gbr;
pub use passport::passport_gbr;
pub use postcode::postcode_gbr;
pub use sort_code::sort_code_gbr;
pub use vehicle_registration::vehicle_registration_gbr;
