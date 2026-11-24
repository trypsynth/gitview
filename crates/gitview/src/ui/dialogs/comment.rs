use std::{rc::Rc, sync::Arc};

use gitview_core::{Client, models::Comment};
use wordsmith::completion::{Completer, Detail, Pool, Search, Speech, Suggestion, Trigger};
use wx_utils::{
	add_ok_cancel_footer, build_ok_cancel_buttons_on, dialog_padding,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::{event::KeyboardEvent, prelude::*};

use crate::ui::{live, notice};

const COMMENT_SIZE: Size = Size { width: 500, height: 200 };

/// `@` and a login, as GitHub mentions someone.
const MENTION: Trigger = Trigger {
	sigil: '@',
	name: "Mention",
	verb: "mentioned",
	// A login is letters, digits and hyphens.
	is_query_char: |ch| ch.is_ascii_alphanumeric() || ch == '-',
	repeats: false,
	detail: Detail::Name,
};

/// Asks for a comment on issue `number` in `repo` (`owner/name`) and posts it, then hands the
/// posted comment to `done`. `initial` starts the text off, such as a quote being replied to.
/// Typing `@` suggests `people`, those in the thread with the most recent first, and then
/// whoever GitHub finds. When posting fails, the dialog comes back with the text still in it.
pub fn post_comment<P: WxWidget + WxEvtHandler + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	repo: String,
	number: u64,
	initial: &str,
	people: Vec<String>,
	done: impl FnOnce(Comment) + 'static,
) {
	let mut text = initial.to_owned();
	spawn_for(&parent, async move {
		// Asked for once, so the signed-in user is never suggested to themselves. Without it,
		// they just might be.
		let me = {
			let client = Arc::clone(&client);
			background_blocking(move || client.login()).await.ok()
		};
		let suggestions = Mentions { people, me, client: Arc::clone(&client) };
		while let Some(body) = show_comment_dialog(&parent, number, &text, &suggestions) {
			let (client, repo, posted) = (Arc::clone(&client), repo.clone(), body.clone());
			match background_blocking(move || client.add_comment(&repo, number, &posted)).await {
				Ok(comment) => {
					done(comment);
					return;
				}
				Err(error) => {
					notice::error(&parent, error, "Could Not Post Comment");
					text = body;
				}
			}
		}
	});
}

/// Who the comment box suggests after `@`.
struct Mentions {
	/// The logins in the thread, best first.
	people: Vec<String>,
	/// The signed-in user's login, when it's known, to leave out.
	me: Option<String>,
	client: Arc<Client>,
}

impl Mentions {
	fn is_me(me: Option<&str>, login: &str) -> bool {
		me.is_some_and(|me| me.eq_ignore_ascii_case(login))
	}

	/// The people in the thread, each once.
	fn at_hand(&self) -> Vec<Suggestion> {
		let mut pool = Pool::default();
		for login in self.people.iter().filter(|login| !Self::is_me(self.me.as_deref(), login)) {
			pool.add(Suggestion::new(login.clone(), ""));
		}
		pool.into_vec()
	}

	/// Asks GitHub for whoever else matches what's typed. A failed search finds nobody, which
	/// leaves the people in the thread.
	fn search(&self) -> Search {
		let (client, me) = (Arc::clone(&self.client), self.me.clone());
		Arc::new(move |_, query| {
			client
				.search_users(query)
				.unwrap_or_default()
				.into_iter()
				.filter(|user| !Self::is_me(me.as_deref(), &user.login))
				.map(|user| Suggestion::new(user.login, user.name.unwrap_or_default()))
				.collect()
		})
	}
}

/// Returns the comment text, or `None` when the user cancels or leaves it blank.
fn show_comment_dialog(parent: &dyn WxWidget, number: u64, initial: &str, mentions: &Mentions) -> Option<String> {
	let dialog = Dialog::builder(parent, &format!("Comment on #{number}")).build();
	let panel = Panel::builder(&dialog).build();
	let padding = dialog_padding(&dialog);
	let label = StaticText::builder(&panel).with_label("&Comment (Ctrl+Enter posts):").build();
	let text = TextCtrl::builder(&panel)
		.with_value(initial)
		.with_style(TextCtrlStyle::MultiLine)
		.with_size(COMMENT_SIZE)
		.build();
	// Shown under the text while there are suggestions. The focus stays in the text, which says
	// the one picked, so the list is only for clicking.
	let suggestions = ListBox::builder(&panel).build();
	let (post_button, cancel_button) = build_ok_cancel_buttons_on(&panel, &dialog, "&Post");
	// Speaks the suggestions. Made after the other controls, so no screen reader takes it for
	// one's label.
	let live_region = live::live_region(&panel);
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
	let speak = Rc::new(move |message: &str, _: Speech| live_region::announce(live_region, message));
	let (completer, timer) = Completer::new(
		&dialog,
		panel,
		text,
		suggestions,
		vec![(MENTION, mentions.at_hand())],
		Some(mentions.search()),
		speak,
	);
	let changed = completer.clone();
	text.on_text_changed(move |_| changed.refresh());
	// Enter, Tab and Escape never reach a text box in a dialog, so the completer sees them here
	// first, and Escape closes the suggestions before the dialog.
	dialog.bind_internal(EventType::CHAR_HOOK, move |event| {
		let key = KeyboardEvent::new(event);
		key.event.skip(!completer.handle_key(&key));
	});
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&text, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add(&suggestions, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, padding);
	add_ok_cancel_footer(content, post_button, cancel_button);
	panel.set_sizer(content, true);
	let frame = BoxSizer::builder(Orientation::Vertical).build();
	frame.add(&panel, 1, SizerFlag::Expand, 0);
	dialog.set_sizer_and_fit(frame, true);
	dialog.centre();
	text.set_focus();
	text.set_insertion_point_end();
	let result = (dialog.show_modal() == ID_OK).then(|| text.get_value()).filter(|body| !body.trim().is_empty());
	timer.stop();
	dialog.destroy();
	result
}
