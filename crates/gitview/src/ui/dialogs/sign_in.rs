use std::{
	cell::{Cell, RefCell},
	rc::Rc,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use gitview_core::auth;
use wx_utils::{dialog_padding, show_error};
use wxdragon::{
	clipboard::Clipboard,
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

use crate::ui::worker;

const TITLE: &str = "Sign In to GitHub";

/// Returns the new access token, or `None` when the user cancels or sign-in fails.
pub fn show_sign_in_dialog(parent: &dyn WxWidget) -> Option<String> {
	let code = match auth::request_device_code() {
		Ok(code) => code,
		Err(error) => {
			show_error(parent, error, TITLE);
			return None;
		}
	};
	let dialog = Dialog::builder(parent, TITLE).build();
	let padding = dialog_padding(&dialog);
	let intro = StaticText::builder(&dialog)
		.with_label("Enter this code on GitHub. Gitview continues when you approve it.")
		.build();
	let code_label = StaticText::builder(&dialog).with_label("Sign-in &code:").build();
	let code_field = TextCtrl::builder(&dialog).with_value(&code.user_code).with_style(TextCtrlStyle::ReadOnly).build();
	let open_button = Button::builder(&dialog).with_label("Copy Code and &Open GitHub").build();
	let cancel_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
	dialog.set_escape_id(ID_CANCEL);
	open_button.set_default();
	let user_code = code.user_code.clone();
	let verification_uri = code.verification_uri.clone();
	open_button.on_click(move |_| {
		Clipboard::get().set_text(&user_code);
		launch_default_browser(&verification_uri, BrowserLaunchFlags::Default);
	});
	let code_row = BoxSizer::builder(Orientation::Horizontal).build();
	code_row.add(&code_label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, padding);
	code_row.add(&code_field, 1, SizerFlag::Expand, 0);
	let button_row = BoxSizer::builder(Orientation::Horizontal).build();
	button_row.add(&open_button, 0, SizerFlag::Right, padding);
	button_row.add(&cancel_button, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&intro, 0, SizerFlag::All, padding);
	content.add_sizer(&code_row, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, padding);
	content.add_sizer(&button_row, 0, SizerFlag::AlignRight | SizerFlag::All, padding);
	dialog.set_sizer_and_fit(content, true);
	let token = Rc::new(RefCell::new(None));
	let closed = Rc::new(Cell::new(false));
	let cancel = Arc::new(AtomicBool::new(false));
	let worker_cancel = Arc::clone(&cancel);
	let done_token = Rc::clone(&token);
	let done_closed = Rc::clone(&closed);
	worker::spawn(
		move || auth::wait_for_token(&code, &worker_cancel),
		move |result| {
			if done_closed.get() {
				return;
			}
			match result {
				Ok(value) => {
					*done_token.borrow_mut() = Some(value);
					dialog.end_modal(ID_OK);
				}
				Err(error) => {
					show_error(&dialog, error, TITLE);
					dialog.end_modal(ID_CANCEL);
				}
			}
		},
	);
	dialog.centre();
	code_field.set_focus();
	dialog.show_modal();
	closed.set(true);
	cancel.store(true, Ordering::Relaxed);
	dialog.destroy();
	token.take()
}
