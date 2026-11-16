//! Settings kept between runs, in `gitview.toml`. A portable copy keeps them beside the
//! executable, and an installed one in the platform's usual folder for them.

use std::{collections::BTreeMap, fs, io, path::PathBuf};

use homeport::Homeport;
use logkeeper::Level;
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "gitview.toml";
/// When set, names the folder settings go in, whatever the install, such as to try a second
/// copy with settings of its own.
const CONFIG_DIR_VARIABLE: &str = "GITVIEW_CONFIG_DIR";
/// Startup, sign-in, the changes the user makes and every problem, without a line per request.
pub const DEFAULT_LOG_LEVEL: Level = Level::Info;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
	pub views: Views,
	pub shortcuts: Shortcuts,
	/// How much goes in the log, by logkeeper's names for its levels, such as `debug`.
	pub log_level: Option<String>,
}

impl Config {
	/// The log level in the settings, or info when they don't name one Gitview knows.
	pub fn log_level(&self) -> Level {
		self.log_level.as_deref().and_then(|level| level.parse().ok()).unwrap_or(DEFAULT_LOG_LEVEL)
	}
}

/// The keys bound differently from Gitview's defaults, by action id, written as Gitview shows
/// them, such as `Ctrl+Alt+G`. `none` leaves an action unbound.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
	/// System-wide hotkeys, which work whatever has focus.
	pub global: BTreeMap<String, String>,
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
	fs::read_to_string(home().config_dir().join(FILE_NAME))
		.ok()
		.and_then(|text| toml::from_str(&text).ok())
		.unwrap_or_default()
}

pub fn save(config: &Config) -> io::Result<()> {
	let path = home().create_config_dir()?.join(FILE_NAME);
	let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
	fs::write(path, text)
}

/// The folder this copy of Gitview keeps its settings in, and its log beside them.
pub fn folder() -> PathBuf {
	home().config_dir()
}

/// Where this copy of Gitview keeps its settings. Linux keeps the lowercase folder name it has
/// always used there.
fn home() -> Homeport {
	let name = if cfg!(any(target_os = "windows", target_os = "macos")) { "Gitview" } else { "gitview" };
	Homeport::new(name).with_config_override(CONFIG_DIR_VARIABLE)
}
