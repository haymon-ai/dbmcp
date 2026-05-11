//! Region-neutral recognizers.

pub(super) use super::Recognizer;

mod api_key;
mod credit_card;
mod crypto;
mod cvv;
mod email;
mod iban;
mod ip;
mod jwt_token;
mod mac_address;
mod phone;
mod private_key;
mod url;

pub use api_key::{api_key_aws_secret, api_key_strong};
pub use credit_card::credit_card;
pub use crypto::crypto;
pub use cvv::cvv;
pub use email::email;
pub use iban::iban;
pub use ip::ip_address;
pub use jwt_token::jwt_token;
pub use mac_address::mac_address;
pub use phone::phone_number;
pub use private_key::private_key;
pub use url::url;
