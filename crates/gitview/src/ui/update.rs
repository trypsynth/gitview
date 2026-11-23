//! Checking GitHub for a newer Gitview, and installing it, through ship-shape.

use std::sync::Arc;

use ship_shape::{
	UpdateChannel, UpdaterConfig,
	ui::{self, CheckTrigger},
};
use wxdragon::prelude::*;

use crate::config::{self, Channel};

const REPOSITORY: &str = "accessware/gitview";
/// The minisign key Gitview's builds are signed with. An update whose signature it doesn't verify
/// is never installed.
const PUBLIC_KEY: &str = "RWSPPY7CuiOcORz9tJEWN3J9/Q1pbDZ03F01ozk+tc4Thfdiv5EQpa1p";

/// Looks for an update on the channel the settings choose. An automatic check only speaks up
/// when there's an update; one the user asked for also says when Gitview is up to date, or that
/// it couldn't tell. `restart` runs once an update is ready to install, and must quit Gitview.
pub fn check(frame: &Frame, automatic: bool, restart: impl FnOnce() + Send + 'static) {
	let settings = config::load().updates;
	let channel = match settings.channel {
		Channel::Stable => UpdateChannel::Stable,
		Channel::Dev => UpdateChannel::Dev,
	};
	let updater = Arc::new(
		UpdaterConfig::new(REPOSITORY, "gitview", "Gitview", PUBLIC_KEY, env!("CARGO_PKG_VERSION"))
			.with_commit(env!("GITVIEW_COMMIT_HASH"))
			.with_install_kind(config::install_kind().into())
			.with_asset_suffix(if cfg!(target_arch = "aarch64") { "-arm64" } else { "-x64" }),
	);
	tracing::info!(channel = ?settings.channel, automatic, "checking for updates");
	let trigger = if automatic { CheckTrigger::Automatic } else { CheckTrigger::Manual };
	ui::run_update_check_with_exit_handler(updater, frame, channel, trigger, restart);
}

/// Lets a copy of Gitview that an update relaunched come to the front, which Windows would
/// otherwise open behind whatever the user was doing. Called once the window is first shown.
pub fn come_forward_after_update(frame: &Frame) {
	ui::bring_to_front_after_update(frame);
}
