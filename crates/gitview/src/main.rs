#![cfg_attr(not(test), windows_subsystem = "windows")]

mod token;
mod ui;

fn main() {
	let _ = wxdragon::main(|_| ui::MainWindow::open());
}
