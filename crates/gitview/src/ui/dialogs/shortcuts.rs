//! Gitview's side of wx-utils' Customize Keyboard Shortcuts dialog: a tab for the main window's
//! keys, and on Windows one for the system-wide hotkeys, which fire over the window too and so
//! can't share a key with it.

use std::collections::BTreeMap;

use wx_utils::shortcuts::{KeyChord, ShortcutModel, TabScope};
use wxdragon::prelude::*;

use crate::{
	config::Shortcuts,
	ui::{hotkeys::GlobalAction, keymap::Command},
};

const WINDOW_TAB: usize = 0;

/// A row in the dialog: something in the window's menus, or a hotkey.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
	Window(Command),
	Global(GlobalAction),
}

/// The shortcuts in the settings, as the dialog edits them.
#[derive(Clone)]
struct Model(Shortcuts);

impl Model {
	/// Where in the settings `row`'s key is kept, and its id there.
	const fn binding(&mut self, row: Row) -> (&mut BTreeMap<String, String>, &'static str) {
		match row {
			Row::Window(command) => (&mut self.0.window, command.id()),
			Row::Global(action) => (&mut self.0.global, action.id()),
		}
	}
}

impl ShortcutModel for Model {
	type Action = Row;

	/// Hotkeys only work on Windows, so their tab only shows there.
	fn tabs(&self) -> Vec<String> {
		let mut tabs = vec!["Main window".to_owned()];
		if cfg!(windows) {
			tabs.push("Global".to_owned());
		}
		tabs
	}

	fn tab_scope(&self, tab: usize) -> TabScope {
		if tab == WINDOW_TAB { TabScope::Mode(0) } else { TabScope::Global }
	}

	fn actions(&self, tab: usize) -> Vec<Row> {
		if tab == WINDOW_TAB {
			Command::ALL.into_iter().map(Row::Window).collect()
		} else {
			GlobalAction::ALL.into_iter().map(Row::Global).collect()
		}
	}

	fn action_name(&self, row: Row) -> String {
		match row {
			Row::Window(command) => command.name(),
			Row::Global(action) => action.name(),
		}
		.to_owned()
	}

	fn chord(&self, _tab: usize, row: Row) -> Option<KeyChord> {
		match row {
			Row::Window(command) => command.chord(&self.0),
			Row::Global(action) => action.chord(&self.0),
		}
	}

	fn set_chord(&mut self, _tab: usize, row: Row, chord: Option<KeyChord>) {
		let chord = chord.map_or_else(|| "none".to_owned(), |chord| chord.to_shortcut_string());
		let (keys, id) = self.binding(row);
		keys.insert(id.to_owned(), chord);
	}

	fn reset_action(&mut self, _tab: usize, row: Row) {
		let (keys, id) = self.binding(row);
		keys.remove(id);
	}

	fn reset_all(&mut self, tab: usize) {
		if tab == WINDOW_TAB {
			self.0.window.clear();
		} else {
			self.0.global.clear();
		}
	}
}

/// Shows the dialog for `shortcuts`, and returns them as the user left them, or `None` when the
/// user cancels.
pub fn show_shortcuts_dialog(parent: &dyn WxWidget, shortcuts: &Shortcuts) -> Option<Shortcuts> {
	wx_utils::shortcuts::prompt_for_shortcuts(parent, &Model(shortcuts.clone())).map(|model| model.0)
}

#[cfg(test)]
mod tests {
	use wx_utils::shortcuts::{KeyChord, ShortcutModel};

	use super::{Model, Row};
	use crate::{
		config::Shortcuts,
		ui::{actions::Action, hotkeys::GlobalAction, keymap::Command},
	};

	#[test]
	fn edits_survive_as_settings_and_reset_goes_back_to_the_default() {
		let mut model = Model(Shortcuts::default());
		let row = Row::Global(GlobalAction::ShowHide);
		model.set_chord(1, row, Some(KeyChord::new(true, false, true, "F12")));
		assert_eq!(model.0.global.get("show_hide").map(String::as_str), Some("Ctrl+Shift+F12"));
		model.set_chord(1, row, None);
		assert_eq!(model.chord(1, row), None, "clearing a key keeps it unbound rather than defaulted");
		model.reset_action(1, row);
		assert_eq!(model.chord(1, row), Some(GlobalAction::ShowHide.default_chord()));
	}

	#[test]
	fn the_window_tab_edits_the_window_keys_alone() {
		let mut model = Model(Shortcuts::default());
		let row = Row::Window(Command::Item(Action::Comment));
		model.set_chord(0, row, Some(KeyChord::new(true, false, false, "M")));
		assert_eq!(model.0.window.get("comment").map(String::as_str), Some("Ctrl+M"));
		assert!(model.0.global.is_empty());
		model.reset_all(0);
		assert!(model.0.window.is_empty());
	}
}
