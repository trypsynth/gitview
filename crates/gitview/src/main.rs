#![cfg_attr(not(test), windows_subsystem = "windows")]

use logkeeper::Logkeeper;

mod config;
mod template;
mod token;
mod ui;

fn main() {
	// Before wxWidgets starts, so a second copy hands over and exits without showing anything. If
	// the check fails, this copy runs on its own, as it would have before.
	let door = doorbell::ring("gitview", &[] as &[&str]);
	if matches!(door, Ok(doorbell::Ring::Delivered)) {
		return;
	}
	// After the check for a running copy, so a second one hands over and exits without touching
	// the first one's log.
	let _log = Logkeeper::new(config::folder(), "gitview")
		.with_crates(["gitview", "gitview_core"])
		.init(config::load().log_level());
	tracing::info!(version = env!("CARGO_PKG_VERSION"), commit = env!("GITVIEW_SHORT_HASH"), "starting");
	let door = match door {
		Ok(doorbell::Ring::First(door)) => Some(door),
		Ok(doorbell::Ring::Delivered) => None,
		Err(error) => {
			tracing::warn!(%error, "couldn't check for a running copy, so starting another won't bring this one forward");
			None
		}
	};
	let _ = wxdragon::main(move |_| ui::MainWindow::open(door));
	tracing::info!("closed");
}
