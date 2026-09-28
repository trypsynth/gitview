use std::sync::Arc;

use gitview_core::Client;
use wx_utils::{add_single_button_footer, dialog_padding, show_error};
use wxdragon::prelude::*;

use crate::ui::{text::body_html, web, worker};

const PAGE_SIZE: Size = Size { width: 700, height: 500 };

/// Loads and shows what a notification points at when it is not an issue, such as a release.
/// `url` is the notification's subject URL, which some kinds leave out.
pub fn open_subject<P: WxWidget + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	heading: String,
	meta: String,
	url: Option<String>,
) {
	let Some(url) = url else {
		show_page(&parent, &heading, &meta, "<p>GitHub keeps no text for this kind.</p>");
		return;
	};
	worker::spawn(
		move || client.subject(&url),
		move |result| match result {
			Ok(details) => {
				let body = body_html(details.body_html.as_deref(), details.body.as_deref());
				show_page(&parent, &heading, &meta, &body);
			}
			Err(error) => show_error(&parent, error, "Could Not Open Notification"),
		},
	);
}

fn show_page(parent: &dyn WxWidget, heading: &str, meta: &str, body: &str) {
	let dialog = Dialog::builder(parent, heading).build();
	let padding = dialog_padding(&dialog);
	let view = web::build(&dialog, move || dialog.end_modal(ID_CANCEL));
	view.set_min_size(PAGE_SIZE);
	web::show(view, heading, meta, body);
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&view, 1, SizerFlag::Expand | SizerFlag::All, padding);
	add_single_button_footer(content, close_button);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	view.set_focus();
	dialog.show_modal();
	dialog.destroy();
}
