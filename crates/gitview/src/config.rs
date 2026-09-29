//! Settings kept between runs, in `gitview.toml` in the platform's usual folder for them.

use std::{env, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "gitview.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
	pub views: Views,
}

/// Which views the main window lists, as view ids.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Views {
	/// Every view in the order chosen, shown or not.
	pub order: Vec<String>,
	pub hidden: Vec<String>,
}

/// The saved settings. A missing or unreadable file gives the defaults, since there is nothing
/// in it worth stopping for.
pub fn load() -> Config {
	path()
		.and_then(|path| fs::read_to_string(path).ok())
		.and_then(|text| toml::from_str(&text).ok())
		.unwrap_or_default()
}

pub fn save(config: &Config) -> io::Result<()> {
	let path = path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no folder for settings was found"))?;
	if let Some(folder) = path.parent() {
		fs::create_dir_all(folder)?;
	}
	let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
	fs::write(path, text)
}

fn path() -> Option<PathBuf> {
	Some(folder()?.join(FILE_NAME))
}

#[cfg(target_os = "windows")]
fn folder() -> Option<PathBuf> {
	Some(PathBuf::from(env::var_os("APPDATA")?).join("Gitview"))
}

#[cfg(target_os = "macos")]
fn folder() -> Option<PathBuf> {
	Some(PathBuf::from(env::var_os("HOME")?).join("Library/Application Support/Gitview"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn folder() -> Option<PathBuf> {
	let base = env::var_os("XDG_CONFIG_HOME")
		.map(PathBuf::from)
		.or_else(|| Some(PathBuf::from(env::var_os("HOME")?).join(".config")))?;
	Some(base.join("gitview"))
}
