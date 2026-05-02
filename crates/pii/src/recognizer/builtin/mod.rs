//! Built-in pattern recognizers shipped by default.
//!
//! Eight recognizers, ported from Presidio's language-agnostic
//! `predefined_recognizers/generic` set, registered in this exact order so
//! overlap-resolution tie-breaks (registration order) are deterministic.

mod all;
mod credit_card;
mod crypto;
mod email;
mod iban;
mod ip;
mod phone;
mod url;
mod us_ssn;

pub use all::all;
pub use credit_card::credit_card;
pub use crypto::crypto;
pub use email::email;
pub use iban::iban;
pub use ip::ip_address;
pub use phone::phone_number;
pub use url::url;
pub use us_ssn::us_ssn;
