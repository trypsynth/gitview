pub mod api;
pub mod auth;
mod error;
pub mod models;

pub use crate::{api::Client, error::Error};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!("gitview");

#[cfg_attr(feature = "uniffi", uniffi::export)]
#[must_use]
pub fn version() -> String {
	env!("CARGO_PKG_VERSION").to_string()
}
