//! Germany-specific recognizers (ISO 3166-1 alpha-3 `DEU`).

pub(super) use super::Recognizer;

mod commercial_register;
mod driving_licence;
mod health_insurance;
mod id_card;
mod license_plate;
mod lifetime_physician_number;
mod medical_practice_id;
mod passport;
mod postcode;
mod social_security;
mod tax_id;
mod tax_number;

pub use commercial_register::commercial_register_deu;
pub use driving_licence::driving_licence_deu;
pub use health_insurance::health_insurance_deu;
pub use id_card::id_card_deu;
pub use license_plate::license_plate_deu;
pub use lifetime_physician_number::lifetime_physician_number_deu;
pub use medical_practice_id::medical_practice_id_deu;
pub use passport::passport_deu;
pub use postcode::postcode_deu;
pub use social_security::social_security_deu;
pub use tax_id::tax_id_deu;
pub use tax_number::tax_number_deu;
