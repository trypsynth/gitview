//! What can be done to the item selected in the main window. The same list fills its context
//! menu and the Item menu, so every action has a place in the menu bar and a shortcut there.

use gitview_core::models::{Issue, Notification, Repository};
use wx_utils::menu_label;
use wxdragon::prelude::*;

use super::keymap::Command;
use crate::config::Shortcuts;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
	Star,
	Unstar,
	Watch,
	Unwatch,
	Copy,
	CopyLink,
	OpenInBrowser,
}

impl Action {
	const ALL: [Self; 17] = [
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
		Self::Star,
		Self::Unstar,
		Self::Watch,
		Self::Unwatch,
		Self::Copy,
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
			Self::Open => "&Open",
			Self::Comment => "&Comment...",
			Self::CloseCompleted => "&Completed",
			Self::CloseNotPlanned => "&Not planned",
			Self::CloseDuplicate => "&Duplicate of...",
			Self::ClosePullRequest => "Clo&se",
			Self::Reopen => "R&eopen",
			Self::MarkRead => "Mark as &read",
			Self::MarkDone => "Mark as &done",
			Self::Unsubscribe => "&Unsubscribe",
			Self::Star => "S&tar",
			Self::Unstar => "Uns&tar",
			Self::Watch => "&Watch",
			Self::Unwatch => "Un&watch",
			Self::Copy => "Cop&y",
			Self::CopyLink => "Copy &link",
			Self::OpenInBrowser => "Open in &browser",
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
			Self::Star => "Star the repository",
			Self::Unstar => "Remove your star",
			Self::Watch => "Get notified of all activity in the repository",
			Self::Unwatch => "Stop watching the repository",
			Self::Copy => "Copy the item as the list says it",
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
	entries.extend([
		Entry::Separator,
		Entry::Action(Action::Copy),
		Entry::Action(Action::CopyLink),
		Entry::Action(Action::OpenInBrowser),
	]);
	entries
}

/// A menu of what `selected` offers, with the keys `shortcuts` binds, or a single disabled line
/// when nothing is selected.
pub fn menu(selected: Option<&Selected>, marks: Marks, shortcuts: &Shortcuts) -> Menu {
	let menu = Menu::builder().build();
	let Some(selected) = selected else {
		menu.append(ID_NOTHING, "No item selected", "", ItemKind::Normal);
		menu.enable_item(ID_NOTHING, false);
		return menu;
	};
	for entry in entries(selected, marks) {
		match entry {
			Entry::Action(action) => append(&menu, action, shortcuts),
			Entry::Submenu(label, actions) => {
				let submenu = Menu::builder().build();
				for action in actions {
					append(&submenu, action, shortcuts);
				}
				menu.append_submenu(submenu, label, "");
			}
			Entry::Separator => menu.append_separator(),
		}
	}
	menu
}

fn append(menu: &Menu, action: Action, shortcuts: &Shortcuts) {
	let label = menu_label(action.label(), &Command::Item(action).accelerator(shortcuts));
	menu.append(action.id(), &label, action.help(), ItemKind::Normal);
}
