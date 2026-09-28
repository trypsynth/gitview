use std::{cell::RefCell, fmt::Write, rc::Rc, sync::Arc, thread};

use gitview_core::{
	Client, Error,
	models::{Comment, Issue},
};
use wx_utils::{dialog_padding, show_error};
use wxdragon::{prelude::*, widgets::WebView};

use super::comment::show_comment_dialog;
use crate::ui::{
	text::{body_html, comment_count},
	web, worker,
};

const ISSUE_SIZE: Size = Size { width: 700, height: 500 };

/// Loads issue or pull request `number` in `repo` (`owner/name`) and shows it with its comments.
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
			Ok((issue, comments)) => show_issue_dialog(&parent, client, repo, issue, comments),
			Err(error) => show_error(&parent, error, "Could Not Open Issue"),
		},
	);
}

fn show_issue_dialog(parent: &dyn WxWidget, client: Arc<Client>, repo: String, issue: Issue, comments: Vec<Comment>) {
	let number = issue.number;
	let dialog = Dialog::builder(parent, &format!("{repo}#{number}: {}", issue.title)).build();
	let padding = dialog_padding(&dialog);
	let view = web::build(&dialog, move || dialog.end_modal(ID_CANCEL));
	view.set_min_size(ISSUE_SIZE);
	let comments = Rc::new(RefCell::new(comments));
	render(view, &issue, &comments.borrow());
	let comment_button = Button::builder(&dialog).with_label("&Comment...").build();
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let closed = Rc::new(std::cell::Cell::new(false));
	let click_closed = Rc::clone(&closed);
	comment_button.on_click(move |_| {
		let Some(body) = show_comment_dialog(&dialog, number) else {
			return;
		};
		let client = Arc::clone(&client);
		let repo = repo.clone();
		let closed = Rc::clone(&click_closed);
		let comments = Rc::clone(&comments);
		let issue = issue.clone();
		worker::spawn(
			move || client.add_comment(&repo, number, &body),
			move |result| {
				if closed.get() {
					return;
				}
				match result {
					Ok(comment) => {
						comments.borrow_mut().push(comment);
						render(view, &issue, &comments.borrow());
						view.set_focus();
					}
					Err(error) => show_error(&dialog, error, "Could Not Post Comment"),
				}
			},
		);
	});
	let button_row = BoxSizer::builder(Orientation::Horizontal).build();
	button_row.add(&comment_button, 0, SizerFlag::empty(), 0);
	button_row.add_stretch_spacer(1);
	button_row.add(&close_button, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&view, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add_sizer(
		&button_row,
		0,
		SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
		padding,
	);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	view.set_focus();
	dialog.show_modal();
	closed.set(true);
	dialog.destroy();
}

fn render(view: WebView, issue: &Issue, comments: &[Comment]) {
	let mut body = body_html(issue.body_html.as_deref(), issue.body.as_deref());
	for comment in comments {
		let _ = write!(
			body,
			"<h2>{} commented on {}</h2>{}",
			web::escape(&comment.user.login),
			web::escape(date(&comment.created_at)),
			body_html(comment.body_html.as_deref(), comment.body.as_deref()),
		);
	}
	let heading = format!("{} #{}", issue.title, issue.number);
	let meta = format!(
		"{}, {}. Opened by {} on {}. {}.",
		kind(issue),
		issue.state,
		issue.user.login,
		date(&issue.created_at),
		comment_count(issue.comments),
	);
	web::show(view, &heading, &meta, &body);
}

const fn kind(issue: &Issue) -> &'static str {
	if issue.is_pull_request() { "Pull request" } else { "Issue" }
}

fn date(timestamp: &str) -> &str {
	timestamp.split('T').next().unwrap_or(timestamp)
}
