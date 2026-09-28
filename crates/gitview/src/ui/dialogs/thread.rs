use std::{cell::RefCell, rc::Rc, sync::Arc, thread};

use gitview_core::{
	Client, Error, html_to_text,
	models::{Comment, Issue},
};
use wx_utils::{dialog_padding, show_error};
use wxdragon::{
	clipboard::Clipboard,
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

use super::{comment::post_comment, page::show_page};
use crate::ui::{
	text::{body_html, comment_count, local_time},
	worker,
};

const THREAD_SIZE: Size = Size { width: 700, height: 450 };
const ID_OPEN_POST: i32 = ID_HIGHEST + 200;
const ID_REPLY: i32 = ID_HIGHEST + 201;
const ID_COPY_TEXT: i32 = ID_HIGHEST + 202;
const ID_COPY_LINK: i32 = ID_HIGHEST + 203;
const ID_OPEN_IN_BROWSER: i32 = ID_HIGHEST + 204;

/// One entry in the thread: the issue's own description, or a comment on it.
#[derive(Clone)]
struct Post {
	/// What the list shows: who wrote it and when, then the whole text, line breaks and all.
	label: String,
	heading: String,
	/// The line under the heading, empty when the heading already says it all.
	meta: String,
	html: String,
	/// The markdown as written, for quoting in a reply.
	markdown: String,
	url: String,
}

impl Post {
	fn new(heading: String, meta: String, html: String, markdown: Option<&str>, url: &str) -> Self {
		let text = html_to_text(&html);
		let text = if text.is_empty() { "No text.".to_owned() } else { text };
		Self {
			label: format!("{}:\n{text}", if meta.is_empty() { &heading } else { &meta }),
			heading,
			meta,
			html,
			markdown: markdown.unwrap_or_default().trim().to_owned(),
			url: url.to_owned(),
		}
	}

	fn of_issue(issue: &Issue) -> Self {
		let kind = if issue.is_pull_request() { "pull request" } else { "issue" };
		Self::new(
			format!("{} #{}", issue.title, issue.number),
			format!("{} opened this {kind} on {}", issue.user.login, local_time(&issue.created_at)),
			body_html(issue.body_html.as_deref(), issue.body.as_deref()),
			issue.body.as_deref(),
			&issue.html_url,
		)
	}

	fn of_comment(comment: &Comment) -> Self {
		Self::new(
			format!("{} commented on {}", comment.user.login, local_time(&comment.created_at)),
			String::new(),
			body_html(comment.body_html.as_deref(), comment.body.as_deref()),
			comment.body.as_deref(),
			&comment.html_url,
		)
	}

	/// The markdown with each line quoted, then a blank line to start the reply on.
	fn quote(&self) -> String {
		let mut quote = String::new();
		for line in self.markdown.lines() {
			quote.push_str("> ");
			quote.push_str(line);
			quote.push('\n');
		}
		quote.push('\n');
		quote
	}

	/// The text without the line saying who wrote it.
	fn text(&self) -> &str {
		self.label.split_once('\n').map_or(self.label.as_str(), |(_, text)| text)
	}
}

/// Loads issue or pull request `number` in `repo` (`owner/name`) and shows its thread.
pub fn open_issue<P: WxWidget + Copy + 'static>(parent: P, client: Arc<Client>, repo: String, number: u64) {
	let fetch_client = Arc::clone(&client);
	let fetch_repo = repo.clone();
	worker::spawn(
		move || {
			thread::scope(|scope| {
				let comments = scope.spawn(|| fetch_client.comments(&fetch_repo, number));
				let issue = fetch_client.issue(&fetch_repo, number)?;
				Ok::<_, Error>((issue, comments.join().expect("comment fetch panicked")?))
			})
		},
		move |result| match result {
			Ok((issue, comments)) => show_thread_dialog(&parent, client, repo, &issue, &comments),
			Err(error) => show_error(&parent, error, "Could Not Open Issue"),
		},
	);
}

fn show_thread_dialog(parent: &dyn WxWidget, client: Arc<Client>, repo: String, issue: &Issue, comments: &[Comment]) {
	let number = issue.number;
	let dialog = Dialog::builder(parent, &format!("{repo}#{number}: {}", issue.title)).build();
	let padding = dialog_padding(&dialog);
	let summary = format!(
		"{} {}, {}.",
		if issue.is_pull_request() { "Pull request" } else { "Issue" },
		issue.state,
		comment_count(issue.comments),
	);
	let label = StaticText::builder(&dialog).with_label(&format!("&Thread. {summary}")).build();
	let list = ListBox::builder(&dialog).with_size(THREAD_SIZE).build();
	// Speaks what copying did. Made after the list, so no screen reader takes it for its label.
	let live_region = StaticText::builder(&dialog).with_label("").with_size(Size::new(0, 0)).build();
	live_region.show(false);
	let posts: Vec<Post> =
		std::iter::once(Post::of_issue(issue)).chain(comments.iter().map(Post::of_comment)).collect();
	for post in &posts {
		list.append(&post.label);
	}
	list.set_selection(0, true);
	let posts = Rc::new(RefCell::new(posts));
	// Open is the default button, so Enter on a post shows it formatted.
	let open_button = Button::builder(&dialog).with_label("&Open").build();
	let comment_button = Button::builder(&dialog).with_label("&Comment...").build();
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	open_button.set_default();
	dialog.set_escape_id(ID_CANCEL);
	// Handlers get a copy of the post, since replying opens a dialog, and a comment finishing
	// meanwhile adds to the list.
	let selected_post = {
		let posts = Rc::clone(&posts);
		move || list.get_selection().and_then(|index| posts.borrow().get(index as usize).cloned())
	};
	let open_post = {
		let selected_post = selected_post.clone();
		move || {
			if let Some(post) = selected_post() {
				show_page(&dialog, &post.heading, &post.meta, &post.html);
			}
		}
	};
	let reply = move |initial: String| {
		let posts = Rc::clone(&posts);
		post_comment(dialog, Arc::clone(&client), repo.clone(), number, &initial, move |comment| {
			let post = Post::of_comment(&comment);
			list.append(&post.label);
			posts.borrow_mut().push(post);
			list.set_selection(list.get_count() - 1, true);
			list.set_focus();
		});
	};
	let open_on_click = open_post.clone();
	open_button.on_click(move |_| open_on_click());
	let open_on_double_click = open_post.clone();
	list.on_item_double_clicked(move |_| open_on_double_click());
	let comment_reply = reply.clone();
	comment_button.on_click(move |_| comment_reply(String::new()));
	bind_post_menu(list, live_region, selected_post, open_post, reply);
	let button_row = BoxSizer::builder(Orientation::Horizontal).build();
	button_row.add(&open_button, 0, SizerFlag::Right, padding);
	button_row.add(&comment_button, 0, SizerFlag::empty(), 0);
	button_row.add_stretch_spacer(1);
	button_row.add(&close_button, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&list, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add_sizer(
		&button_row,
		0,
		SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
		padding,
	);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	list.set_focus();
	dialog.show_modal();
	dialog.destroy();
}

/// The context menu on a post, and what its choices do.
fn bind_post_menu(
	list: ListBox,
	live_region: StaticText,
	selected_post: impl Fn() -> Option<Post> + 'static,
	open_post: impl Fn() + 'static,
	reply: impl Fn(String) + 'static,
) {
	// ListBox has no menu event methods of its own, so the events are bound directly.
	list.bind_internal(EventType::CONTEXT_MENU, move |_| {
		let mut menu = Menu::builder().build();
		menu.append(ID_OPEN_POST, "&Open\tEnter", "Show this post formatted", ItemKind::Normal);
		menu.append(ID_REPLY, "&Reply with quote...", "Comment, quoting this post", ItemKind::Normal);
		menu.append_separator();
		menu.append(ID_COPY_TEXT, "Copy &text", "Copy this post's text", ItemKind::Normal);
		menu.append(ID_COPY_LINK, "Copy &link", "Copy the link to this post", ItemKind::Normal);
		menu.append(ID_OPEN_IN_BROWSER, "Open in &browser", "Show this post on GitHub", ItemKind::Normal);
		list.popup_menu(&mut menu, None);
		menu.destroy_menu();
	});
	list.bind_internal(EventType::MENU, move |event| {
		let id = event.get_id();
		if id == ID_OPEN_POST {
			open_post();
			return;
		}
		let Some(post) = selected_post() else {
			return;
		};
		let copy = |text: &str, done: &str| {
			let message = if Clipboard::get().set_text(text) { done } else { "Gitview couldn't copy that." };
			live_region::announce(live_region, message);
		};
		match id {
			ID_REPLY => reply(post.quote()),
			ID_COPY_TEXT => copy(post.text(), "Text copied."),
			ID_COPY_LINK => copy(&post.url, "Link copied."),
			ID_OPEN_IN_BROWSER => {
				launch_default_browser(&post.url, BrowserLaunchFlags::Default);
			}
			_ => event.skip(true),
		}
	});
}
