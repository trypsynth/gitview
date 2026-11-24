//! The keys bound to what the main window's menus do, which the user can change in Customize
//! Keyboard Shortcuts.

use key_chord::KeyChord;

use super::actions::Action;
use crate::config::Shortcuts;

/// Something in the main window's menus that a key can be bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
	/// What the selected item offers. Star and Unstar share a key, as do Watch and Unwatch,
	/// since only one of each pair is ever offered.
	Item(Action),
	GoToRepository,
	GoToUser,
	/// Moves the view on screen up the Views list, or down it.
	MoveViewUp,
	MoveViewDown,
	Refresh,
	Settings,
	CustomizeShortcuts,
	EditProfile,
	SignOut,
	CheckForUpdates,
	ViewHelp,
}

impl Command {
	/// Every command, in the order Customize Keyboard Shortcuts lists them.
	pub const ALL: [Self; 27] = [
		Self::Item(Action::Open),
		Self::Item(Action::Comment),
		Self::Item(Action::CloseCompleted),
		Self::Item(Action::CloseNotPlanned),
		Self::Item(Action::CloseDuplicate),
		Self::Item(Action::ClosePullRequest),
		Self::Item(Action::Reopen),
		Self::Item(Action::Merge),
		Self::Item(Action::MarkRead),
		Self::Item(Action::MarkDone),
		Self::Item(Action::Unsubscribe),
		Self::Item(Action::Star),
		Self::Item(Action::Watch),
		Self::Item(Action::Copy),
		Self::Item(Action::CopyLink),
		Self::Item(Action::OpenInBrowser),
		Self::GoToRepository,
		Self::GoToUser,
		Self::MoveViewUp,
		Self::MoveViewDown,
		Self::Refresh,
		Self::Settings,
		Self::CustomizeShortcuts,
		Self::EditProfile,
		Self::SignOut,
		Self::CheckForUpdates,
		Self::ViewHelp,
	];

	/// What the settings file calls it.
	pub const fn id(self) -> &'static str {
		match self {
			Self::Item(action) => match action {
				Action::Open => "open",
				Action::Comment => "comment",
				Action::CloseCompleted => "close_completed",
				Action::CloseNotPlanned => "close_not_planned",
				Action::CloseDuplicate => "close_duplicate",
				Action::ClosePullRequest => "close_pull_request",
				Action::Reopen => "reopen",
				Action::Merge => "merge",
				Action::MarkRead => "mark_read",
				Action::MarkDone => "mark_done",
				Action::Unsubscribe => "unsubscribe",
				Action::Star | Action::Unstar => "star",
				Action::Watch | Action::Unwatch => "watch",
				Action::Copy => "copy",
				Action::CopyLink => "copy_link",
				Action::OpenInBrowser => "open_in_browser",
			},
			Self::GoToRepository => "go_to_repository",
			Self::GoToUser => "go_to_user",
			Self::MoveViewUp => "move_view_up",
			Self::MoveViewDown => "move_view_down",
			Self::Refresh => "refresh",
			Self::Settings => "settings",
			Self::CustomizeShortcuts => "customize_shortcuts",
			Self::EditProfile => "edit_profile",
			Self::SignOut => "sign_out",
			Self::CheckForUpdates => "check_for_updates",
			Self::ViewHelp => "view_help",
		}
	}

	/// What Customize Keyboard Shortcuts calls it.
	pub const fn name(self) -> &'static str {
		match self {
			Self::Item(action) => match action {
				Action::Open => "Open",
				Action::Comment => "Comment",
				Action::CloseCompleted => "Close as completed",
				Action::CloseNotPlanned => "Close as not planned",
				Action::CloseDuplicate => "Close as duplicate",
				Action::ClosePullRequest => "Close pull request",
				Action::Reopen => "Reopen",
				Action::Merge => "Merge",
				Action::MarkRead => "Mark as read",
				Action::MarkDone => "Mark as done",
				Action::Unsubscribe => "Unsubscribe",
				Action::Star | Action::Unstar => "Star or unstar",
				Action::Watch | Action::Unwatch => "Watch or unwatch",
				Action::Copy => "Copy",
				Action::CopyLink => "Copy link",
				Action::OpenInBrowser => "Open in browser",
			},
			Self::GoToRepository => "Go to repository",
			Self::GoToUser => "Go to user",
			Self::MoveViewUp => "Move view up",
			Self::MoveViewDown => "Move view down",
			Self::Refresh => "Refresh",
			Self::Settings => "Settings",
			Self::CustomizeShortcuts => "Customize keyboard shortcuts",
			Self::EditProfile => "Edit profile",
			Self::SignOut => "Sign out",
			Self::CheckForUpdates => "Check for updates",
			Self::ViewHelp => "View help",
		}
	}

	/// The key it has until the user changes it, if any.
	pub fn default_chord(self) -> Option<KeyChord> {
		let (ctrl, shift, key) = match self {
			Self::Item(Action::Open) => (false, false, "Enter"),
			Self::Item(Action::Comment) => (true, false, "R"),
			Self::Item(Action::MarkDone) => (false, false, "Delete"),
			Self::Item(Action::Copy) => (true, false, "C"),
			Self::Item(Action::CopyLink) => (true, true, "C"),
			Self::Item(Action::OpenInBrowser) => (true, false, "B"),
			Self::GoToRepository => (true, false, "G"),
			Self::GoToUser => (true, false, "U"),
			Self::MoveViewUp => (false, true, "Up"),
			Self::MoveViewDown => (false, true, "Down"),
			Self::Refresh => (false, false, "F5"),
			Self::Settings => (true, false, ","),
			Self::EditProfile => (true, true, "E"),
			Self::ViewHelp => (false, false, "F1"),
			_ => return None,
		};
		Some(KeyChord::new(ctrl, false, shift, key))
	}

	/// The key `shortcuts` binds this to, or the default when the user hasn't changed it.
	pub fn chord(self, shortcuts: &Shortcuts) -> Option<KeyChord> {
		shortcuts.window.get(self.id()).map_or_else(|| self.default_chord(), |chord| KeyChord::parse(chord))
	}

	/// The key as a menu shows it after the item's text, or nothing when it has none.
	pub fn accelerator(self, shortcuts: &Shortcuts) -> String {
		self.chord(shortcuts).map(|chord| chord.to_shortcut_string()).unwrap_or_default()
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use super::{Action, Command};
	use crate::config::Shortcuts;

	#[test]
	fn the_defaults_are_the_keys_the_menus_always_had() {
		let shortcuts = Shortcuts::default();
		let key = |command: Command| command.accelerator(&shortcuts);
		assert_eq!(key(Command::Item(Action::Open)), "Enter");
		assert_eq!(key(Command::Item(Action::Comment)), "Ctrl+R");
		assert_eq!(key(Command::Item(Action::MarkDone)), "Delete");
		assert_eq!(key(Command::Item(Action::Copy)), "Ctrl+C");
		assert_eq!(key(Command::Item(Action::CopyLink)), "Ctrl+Shift+C");
		assert_eq!(key(Command::Item(Action::OpenInBrowser)), "Ctrl+B");
		assert_eq!(key(Command::GoToRepository), "Ctrl+G");
		assert_eq!(key(Command::GoToUser), "Ctrl+U");
		assert_eq!(key(Command::MoveViewUp), "Shift+Up");
		assert_eq!(key(Command::MoveViewDown), "Shift+Down");
		assert_eq!(key(Command::Refresh), "F5");
		assert_eq!(key(Command::Settings), "Ctrl+,");
		assert_eq!(key(Command::EditProfile), "Ctrl+Shift+E");
		assert_eq!(key(Command::SignOut), "");
	}

	#[test]
	fn each_pair_of_toggles_shares_a_key_and_every_command_has_its_own_id() {
		assert_eq!(Command::Item(Action::Unstar).id(), Command::Item(Action::Star).id());
		assert_eq!(Command::Item(Action::Unwatch).id(), Command::Item(Action::Watch).id());
		let ids: HashSet<_> = Command::ALL.iter().map(|command| command.id()).collect();
		assert_eq!(ids.len(), Command::ALL.len());
	}

	#[test]
	fn a_changed_key_replaces_the_default() {
		let mut shortcuts = Shortcuts::default();
		shortcuts.window.insert("refresh".to_owned(), "Ctrl+F5".to_owned());
		shortcuts.window.insert("open".to_owned(), "none".to_owned());
		assert_eq!(Command::Refresh.accelerator(&shortcuts), "Ctrl+F5");
		assert_eq!(Command::Item(Action::Open).accelerator(&shortcuts), "");
	}
}
