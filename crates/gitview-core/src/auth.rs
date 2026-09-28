use std::{
	sync::atomic::{AtomicBool, Ordering},
	thread,
	time::Duration,
};

use serde::Deserialize;

use crate::Error;

pub const CLIENT_ID: &str = "Ov23liiwcN8PgLP9oscM";
const SCOPES: &str = "repo notifications read:org";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
// GitHub's device flow docs ask clients to add this to the poll interval on every `slow_down`.
const SLOW_DOWN_PENALTY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
	pub device_code: String,
	pub user_code: String,
	pub verification_uri: String,
	pub expires_in: u64,
	pub interval: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TokenResponse {
	Token { access_token: String },
	Error { error: String },
}

pub fn request_device_code() -> Result<DeviceCode, Error> {
	Ok(ureq::post(DEVICE_CODE_URL)
		.header("Accept", "application/json")
		.send_form([("client_id", CLIENT_ID), ("scope", SCOPES)])?
		.body_mut()
		.read_json()?)
}

/// Blocks until the user enters the code on GitHub, denies it, the code expires, or `cancel` is set.
pub fn wait_for_token(code: &DeviceCode, cancel: &AtomicBool) -> Result<String, Error> {
	let mut interval = Duration::from_secs(code.interval);
	loop {
		thread::sleep(interval);
		if cancel.load(Ordering::Relaxed) {
			return Err(Error::Cancelled);
		}
		let response: TokenResponse = ureq::post(ACCESS_TOKEN_URL)
			.header("Accept", "application/json")
			.send_form([
				("client_id", CLIENT_ID),
				("device_code", code.device_code.as_str()),
				("grant_type", DEVICE_GRANT_TYPE),
			])?
			.body_mut()
			.read_json()?;
		match response {
			TokenResponse::Token { access_token } => return Ok(access_token),
			TokenResponse::Error { error } => match error.as_str() {
				"authorization_pending" => {}
				"slow_down" => interval += SLOW_DOWN_PENALTY,
				"expired_token" => return Err(Error::CodeExpired),
				"access_denied" => return Err(Error::AccessDenied),
				_ => return Err(Error::Auth(error)),
			},
		}
	}
}
