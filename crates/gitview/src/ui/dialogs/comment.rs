use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding};
use wxdragon::prelude::*;

const COMMENT_SIZE: Size = Size { width: 500, height: 200 };

/// Returns the comment text, or `None` when the user cancels or leaves it blank.
pub fn show_comment_dialog(parent: &dyn WxWidget, number: u64) -> Option<String> {
	let dialog = Dialog::builder(parent, &format!("Comment on #{number}")).build();
	let padding = dialog_padding(&dialog);
	let label = StaticText::builder(&dialog).with_label("&Comment (Ctrl+Enter posts):").build();
	let text = TextCtrl::builder(&dialog).with_style(TextCtrlStyle::MultiLine).with_size(COMMENT_SIZE).build();
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
	let result = (dialog.show_modal() == ID_OK).then(|| text.get_value()).filter(|body| !body.trim().is_empty());
	dialog.destroy();
	result
}
