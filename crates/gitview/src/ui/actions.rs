//! What can be done to the item selected in the main window. The same list fills its context
//! menu and the Item menu, so every action has a place in the menu bar and a shortcut there.

use gitview_core::models::{Issue, Notification, Repository};
use wxdragon::prelude::*;

const ID_NOTHING: i32 = ID_HIGHEST + 99;
const ID_FIRST: i32 = ID_HIGHEST + 100;

/// A copy of the selected item, so an action can run without holding the window's state.
#[derive(Clone)]
pub enum Selected {
	Notification(Notification),
	Repository(Repository),
	Issue(Issue),
}

impl Selected {
	/// The `owner/name` and number of the issue or pull request this is, or is about.
	pub fn issue(&self) -> Option<(String, u64)> {
		match self {
			Self::Notification(notification) => {
				Some((notification.repository.full_name.clone(), notification.issue_number()?))
			}
			Self::Issue(issue) => Some((issue.repository()?.to_owned(), issue.number)),
			Self::Repository(_) => None,
		}
	}

	pub fn is_pull_request(&self) -> bool {
		match self {
			Self::Notification(notification) => notification.is_pull_request(),
			Self::Issue(issue) => issue.is_pull_request(),
			Self::Repository(_) => false,
		}
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
	Open,
	Comment,
	CloseCompleted,
	CloseNotPlanned,
	CloseDuplicate,
	ClosePullRequest,
	Reopen,
	MarkRead,
	MarkDone,
	Unsubscribe,
	Issues,
	PullRequests,
	Star,
	Unstar,
	Watch,
	Unwatch,
	CopyLink,
	OpenInBrowser,
}

impl Action {
	const ALL: [Self; 18] = [
		Self::Open,
		Self::Comment,
		Self::CloseCompleted,
		Self::CloseNotPlanned,
		Self::CloseDuplicate,
		Self::ClosePullRequest,
		Self::Reopen,
		Self::MarkRead,
		Self::MarkDone,
		Self::Unsubscribe,
		Self::Issues,
		Self::PullRequests,
		Self::Star,
		Self::Unstar,
		Self::Watch,
		Self::Unwatch,
		Self::CopyLink,
		Self::OpenInBrowser,
	];

	pub fn from_id(id: i32) -> Option<Self> {
		Self::ALL.into_iter().find(|action| action.id() == id)
	}

	fn id(self) -> i32 {
		ID_FIRST + i32::try_from(Self::ALL.iter().position(|action| *action == self).unwrap_or_default()).unwrap_or(0)
	}

	const fn label(self) -> &'static str {
		match self {
			Self::Open => "&Open\tEnter",
			Self::Comment => "&Comment...\tCtrl+R",
			Self::CloseCompleted => "&Completed",
			Self::CloseNotPlanned => "&Not planned",
			Self::CloseDuplicate => "&Duplicate of...",
			Self::ClosePullRequest => "Clo&se",
			Self::Reopen => "R&eopen",
			Self::MarkRead => "Mark as &read",
			Self::MarkDone => "Mark as &done\tDelete",
			Self::Unsubscribe => "&Unsubscribe",
			Self::Issues => "&Issues",
			Self::PullRequests => "&Pull requests",
			Self::Star => "S&tar",
			Self::Unstar => "Uns&tar",
			Self::Watch => "&Watch",
			Self::Unwatch => "Un&watch",
			Self::CopyLink => "Copy &link\tCtrl+Shift+C",
			Self::OpenInBrowser => "Open in &browser\tCtrl+B",
		}
	}

	const fn help(self) -> &'static str {
		match self {
			Self::Open => "Open the selected item",
			Self::Comment => "Add a comment",
			Self::CloseCompleted => "Close as done",
			Self::CloseNotPlanned => "Close as something that won't be done",
			Self::CloseDuplicate => "Close as a copy of another issue",
			Self::ClosePullRequest => "Close without merging",
			Self::Reopen => "Open again",
			Self::MarkRead => "Mark the notification as read",
			Self::MarkDone => "Remove the notification from your inbox",
			Self::Unsubscribe => "Stop notifications for this thread",
			Self::Issues => "List the repository's issues",
			Self::PullRequests => "List the repository's pull requests",
			Self::Star => "Star the repository",
			Self::Unstar => "Remove your star",
			Self::Watch => "Get notified of all activity in the repository",
			Self::Unwatch => "Stop watching the repository",
			Self::CopyLink => "Copy the link to the item",
			Self::OpenInBrowser => "Show the item on GitHub",
		}
	}
}

enum Entry {
	Action(Action),
	Submenu(&'static str, Vec<Action>),
	Separator,
}

/// Whether a repository is starred and watched, which decides which way its toggles go.
#[derive(Clone, Copy, Default)]
pub struct Marks {
	pub starred: bool,
	pub watching: bool,
}

/// The actions `selected` offers.
fn entries(selected: &Selected, marks: Marks) -> Vec<Entry> {
	let mut entries = vec![Entry::Action(Action::Open)];
	if let Selected::Repository(_) = selected {
		entries.extend([
			Entry::Action(Action::Issues),
			Entry::Action(Action::PullRequests),
			Entry::Separator,
			Entry::Action(if marks.starred { Action::Unstar } else { Action::Star }),
			Entry::Action(if marks.watching { Action::Unwatch } else { Action::Watch }),
		]);
	}
	if selected.issue().is_some() {
		entries.extend([Entry::Action(Action::Comment), Entry::Separator]);
		let close = if selected.is_pull_request() {
			Entry::Action(Action::ClosePullRequest)
		} else {
			Entry::Submenu("Clo&se as", vec![Action::CloseCompleted, Action::CloseNotPlanned, Action::CloseDuplicate])
		};
		// A notification does not say whether its issue is open, so it offers both.
		match selected {
			Selected::Issue(issue) if !issue.is_open() => entries.push(Entry::Action(Action::Reopen)),
			Selected::Issue(_) => entries.push(close),
			_ => entries.extend([close, Entry::Action(Action::Reopen)]),
		}
	}
	if let Selected::Notification(notification) = selected {
		entries.push(Entry::Separator);
		if notification.unread {
			entries.push(Entry::Action(Action::MarkRead));
		}
		entries.extend([Entry::Action(Action::MarkDone), Entry::Action(Action::Unsubscribe)]);
	}
	entries.extend([Entry::Separator, Entry::Action(Action::CopyLink), Entry::Action(Action::OpenInBrowser)]);
	entries
}

/// A menu of what `selected` offers, or a single disabled line when nothing is selected.
pub fn menu(selected: Option<&Selected>, marks: Marks) -> Menu {
	let menu = Menu::builder().build();
	let Some(selected) = selected else {
		menu.append(ID_NOTHING, "No item selected", "", ItemKind::Normal);
		menu.enable_item(ID_NOTHING, false);
		return menu;
	};
	for entry in entries(selected, marks) {
		match entry {
			Entry::Action(action) => append(&menu, action),
			Entry::Submenu(label, actions) => {
				let submenu = Menu::builder().build();
				for action in actions {
					append(&submenu, action);
				}
				menu.append_submenu(submenu, label, "");
			}
			Entry::Separator => menu.append_separator(),
		}
	}
	menu
}

fn append(menu: &Menu, action: Action) {
	menu.append(action.id(), action.label(), action.help(), ItemKind::Normal);
}
