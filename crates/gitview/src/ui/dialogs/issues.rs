use std::{rc::Rc, sync::Arc};

use gitview_core::{
	Client,
	models::{Issue, IssueState},
};
use wx_utils::{
	dialog_padding, show_error,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use super::{open_issue, open_on_enter};
use crate::ui::text::comment_count;

const LIST_SIZE: Size = Size { width: 600, height: 400 };

/// Which of a repository's issues and pull requests to list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Listing {
	Issues,
	PullRequests,
}

impl Listing {
	const fn title(self) -> &'static str {
		match self {
			Self::Issues => "Issues",
			Self::PullRequests => "Pull Requests",
		}
	}

	const fn includes(self, issue: &Issue) -> bool {
		match self {
			Self::Issues => !issue.is_pull_request(),
			Self::PullRequests => issue.is_pull_request(),
		}
	}
}

/// Loads the issues and pull requests in `repo` (`owner/name`) and lists the ones `listing` asks for.
pub fn open_issues<P: WxWidget + WxEvtHandler + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	repo: String,
	state: IssueState,
	listing: Listing,
) {
	let (fetch_client, fetch_repo) = (Arc::clone(&client), repo.clone());
	let fetch = background_blocking(move || fetch_client.issues(&fetch_repo, state));
	spawn_for(&parent, async move {
		match fetch.await {
			// GitHub lists both kinds together, so the one not asked for is dropped here.
			Ok(issues) => {
				let issues = issues.into_iter().filter(|issue| listing.includes(issue)).collect();
				show_issues_dialog(&parent, client, repo, listing, issues);
			}
			Err(error) => show_error(&parent, error, "Could Not Load Issues"),
		}
	});
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
	// Enter or a double-click on an issue opens its thread.
	let open_selected = Rc::new(move || {
		if let Some(issue) = list.get_selection().and_then(|index| issues.get(index as usize)) {
			open_issue(dialog, Arc::clone(&client), repo.clone(), issue.number);
		}
	});
	let open_on_double_click = Rc::clone(&open_selected);
	list.on_item_double_clicked(move |_| open_on_double_click());
	open_on_enter(dialog, list, move || open_selected());
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&list, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add(
		&close_button,
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
