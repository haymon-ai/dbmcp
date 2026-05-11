//! United States-specific recognizers (ISO 3166-1 alpha-3 `USA`).

pub(super) use super::Recognizer;

mod bank_account;
mod driver_license;
mod itin;
mod mbi;
mod medical_license;
mod npi;
mod passport;
mod routing_number;
mod ssn;
mod tax_id_ein;

pub use bank_account::bank_account_usa;
pub use driver_license::driver_license_usa;
pub use itin::itin_usa;
pub use mbi::mbi_usa;
pub use medical_license::medical_license_usa;
pub use npi::npi_usa;
pub use passport::passport_usa;
pub use routing_number::routing_number_usa;
pub use ssn::ssn_usa;
pub use tax_id_ein::tax_id_ein_usa;
