//! German-specific recognizers.

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

pub use commercial_register::commercial_register_de;
pub use driving_licence::driving_licence_de;
pub use health_insurance::health_insurance_de;
pub use id_card::id_card_de;
pub use license_plate::license_plate_de;
pub use lifetime_physician_number::lifetime_physician_number_de;
pub use medical_practice_id::medical_practice_id_de;
pub use passport::passport_de;
pub use postcode::postcode_de;
pub use social_security::social_security_de;
pub use tax_id::tax_id_de;
pub use tax_number::tax_number_de;
