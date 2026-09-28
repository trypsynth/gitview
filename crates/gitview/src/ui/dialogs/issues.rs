use std::{rc::Rc, sync::Arc};

use gitview_core::{
	Client,
	models::{Issue, IssueState},
};
use wx_utils::{dialog_padding, show_error};
use wxdragon::prelude::*;

use super::open_issue;
use crate::ui::{text::comment_count, worker};

const LIST_SIZE: Size = Size { width: 600, height: 400 };

/// Which of a repository's issues and pull requests to list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Listing {
	Both,
	Issues,
	PullRequests,
}

impl Listing {
	const fn title(self) -> &'static str {
		match self {
			Self::Both => "Issues and Pull Requests",
			Self::Issues => "Issues",
			Self::PullRequests => "Pull Requests",
		}
	}

	const fn includes(self, issue: &Issue) -> bool {
		match self {
			Self::Both => true,
			Self::Issues => !issue.is_pull_request(),
			Self::PullRequests => issue.is_pull_request(),
		}
	}
}

/// Loads the issues and pull requests in `repo` (`owner/name`) and lists the ones `listing` asks for.
pub fn open_issues<P: WxWidget + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	repo: String,
	state: IssueState,
	listing: Listing,
) {
	let fetch_client = Arc::clone(&client);
	let fetch_repo = repo.clone();
	worker::spawn(
		// GitHub lists both kinds together, so the one not asked for is dropped here.
		move || {
			fetch_client
				.issues(&fetch_repo, state)
				.map(|issues| issues.into_iter().filter(|issue| listing.includes(issue)).collect())
		},
		move |result| match result {
			Ok(issues) => show_issues_dialog(&parent, client, repo, listing, issues),
			Err(error) => show_error(&parent, error, "Could Not Load Issues"),
		},
	);
}

fn show_issues_dialog(parent: &dyn WxWidget, client: Arc<Client>, repo: String, listing: Listing, issues: Vec<Issue>) {
	let dialog = Dialog::builder(parent, &format!("{repo} {}", listing.title())).build();
	let padding = dialog_padding(&dialog);
	let label = StaticText::builder(&dialog).with_label(&format!("&{}:", listing.title())).build();
	let list = ListBox::builder(&dialog).with_size(LIST_SIZE).build();
	for issue in &issues {
		list.append(&issue_label(issue));
	}
	if issues.is_empty() {
		list.append("Nothing here.");
	}
	list.set_selection(0, true);
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let button_row = BoxSizer::builder(Orientation::Horizontal).build();
	// Nothing to open, so the button would only be a dead tab stop.
	if !issues.is_empty() {
		let open_button = Button::builder(&dialog).with_label("&Open").build();
		open_button.set_default();
		let open_selected = Rc::new(move || {
			if let Some(issue) = list.get_selection().and_then(|index| issues.get(index as usize)) {
				open_issue(dialog, Arc::clone(&client), repo.clone(), issue.number);
			}
		});
		let open_on_click = Rc::clone(&open_selected);
		open_button.on_click(move |_| open_on_click());
		list.on_item_double_clicked(move |_| open_selected());
		button_row.add(&open_button, 0, SizerFlag::Right, padding);
	}
	button_row.add(&close_button, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&list, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add_sizer(
		&button_row,
		0,
		SizerFlag::AlignRight | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
		padding,
	);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	list.set_focus();
	dialog.show_modal();
	dialog.destroy();
}

fn issue_label(issue: &Issue) -> String {
	let kind = if issue.is_pull_request() { ", pull request" } else { "" };
	format!("#{} {}, by {}, {}{kind}", issue.number, issue.title, issue.user.login, comment_count(issue.comments))
}
