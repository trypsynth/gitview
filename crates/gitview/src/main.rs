#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod token;
mod ui;

fn main() {
	let _ = wxdragon::main(|_| ui::MainWindow::open());
}
