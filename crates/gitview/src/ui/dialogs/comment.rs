use std::sync::Arc;

use gitview_core::{Client, models::Comment};
use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding, show_error};
use wxdragon::prelude::*;

use crate::ui::worker;

const COMMENT_SIZE: Size = Size { width: 500, height: 200 };

/// Asks for a comment on issue `number` in `repo` (`owner/name`) and posts it, then hands the
/// posted comment to `done`. `initial` starts the text off, such as a quote being replied to.
/// When posting fails, the dialog comes back with the text still in it.
pub fn post_comment<P: WxWidget + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	repo: String,
	number: u64,
	initial: &str,
	done: impl FnOnce(Comment) + 'static,
) {
	let Some(body) = show_comment_dialog(&parent, number, initial) else {
		return;
	};
	let post_client = Arc::clone(&client);
	let post_repo = repo.clone();
	let post_body = body.clone();
	worker::spawn(
		move || post_client.add_comment(&post_repo, number, &post_body),
		move |result| match result {
			Ok(comment) => done(comment),
			Err(error) => {
				show_error(&parent, error, "Could Not Post Comment");
				post_comment(parent, client, repo, number, &body, done);
			}
		},
	);
}

/// Returns the comment text, or `None` when the user cancels or leaves it blank.
fn show_comment_dialog(parent: &dyn WxWidget, number: u64, initial: &str) -> Option<String> {
	let dialog = Dialog::builder(parent, &format!("Comment on #{number}")).build();
	let padding = dialog_padding(&dialog);
	let label = StaticText::builder(&dialog).with_label("&Comment (Ctrl+Enter posts):").build();
	let text = TextCtrl::builder(&dialog)
		.with_value(initial)
		.with_style(TextCtrlStyle::MultiLine)
		.with_size(COMMENT_SIZE)
		.build();
	let (post_button, cancel_button) = build_ok_cancel_buttons(&dialog, "&Post");
	text.on_key_down(move |event| {
		if let WindowEventData::Keyboard(ref key) = event
			&& key.control_down()
			&& key.get_key_code() == Some(WXK_RETURN)
		{
			dialog.end_modal(ID_OK);
			return;
		}
		event.skip(true);
	});
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&text, 1, SizerFlag::Expand | SizerFlag::All, padding);
	add_ok_cancel_footer(content, post_button, cancel_button);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	text.set_focus();
	text.set_insertion_point_end();
	let result = (dialog.show_modal() == ID_OK).then(|| text.get_value()).filter(|body| !body.trim().is_empty());
	dialog.destroy();
	result
}
