mod comment;
mod issues;
mod page;
mod profile;
mod repository;
mod settings;
#[cfg(windows)]
mod shortcuts;
mod sign_in;
mod thread;

pub use comment::post_comment;
pub use issues::{Listing, open_issues};
pub use page::open_subject;
pub use profile::edit_profile;
pub use repository::open_repository;
pub use settings::show_settings_dialog;
#[cfg(windows)]
pub use shortcuts::show_shortcuts_dialog;
pub use sign_in::show_sign_in_dialog;
pub use thread::open_issue;
use wxdragon::prelude::*;

/// Runs `open` when Enter is pressed in `list`, so its items open without an Open button taking a
/// place in the tab order. Every other key, and Enter anywhere else, goes on as usual.
///
/// The dialog watches for it rather than the list: a dialog takes Enter for its default button
/// before the list would see it.
fn open_on_enter(dialog: Dialog, list: ListBox, open: impl Fn() + 'static) {
	dialog.on_char_hook(move |event| {
		if is_plain_enter(&event) && list.has_focus() {
			open();
		} else {
			event.skip(true);
		}
	});
}

/// Whether `event` is Enter pressed with no modifier.
fn is_plain_enter(event: &WindowEventData) -> bool {
	matches!(event, WindowEventData::Keyboard(key)
		if matches!(key.get_key_code(), Some(WXK_RETURN | WXK_NUMPAD_ENTER))
			&& !(key.control_down() || key.alt_down() || key.shift_down()))
}
