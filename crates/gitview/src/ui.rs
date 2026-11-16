mod actions;
mod dialogs;
mod hotkeys;
mod live;
mod main_window;
mod notice;
mod sectioned;
mod text;
#[cfg(not(target_os = "macos"))]
mod tray;
mod view;
mod visibility;
mod web;

pub use main_window::MainWindow;
