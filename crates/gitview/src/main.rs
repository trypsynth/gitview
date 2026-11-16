#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod token;
mod ui;

fn main() {
	// Before wxWidgets starts, so a second copy hands over and exits without showing anything. If
	// the check fails, this copy runs on its own, as it would have before.
	let door = match doorbell::ring("gitview", &[] as &[&str]) {
		Ok(doorbell::Ring::Delivered) => return,
		Ok(doorbell::Ring::First(door)) => Some(door),
		Err(_) => None,
	};
	let _ = wxdragon::main(move |_| ui::MainWindow::open(door));
}
