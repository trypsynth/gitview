//! Settings kept between runs, in `gitview.toml`. A portable copy keeps them beside the
//! executable, and an installed one in the platform's usual folder for them.

use std::{fs, io};

use homeport::Homeport;
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "gitview.toml";
/// When set, names the folder settings go in, whatever the install, such as to try a second
/// copy with settings of its own.
const CONFIG_DIR_VARIABLE: &str = "GITVIEW_CONFIG_DIR";

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

/// Where this copy of Gitview keeps its settings. Linux keeps the lowercase folder name it has
/// always used there.
fn home() -> Homeport {
	let name = if cfg!(any(target_os = "windows", target_os = "macos")) { "Gitview" } else { "gitview" };
	Homeport::new(name).with_config_override(CONFIG_DIR_VARIABLE)
}
