//! Gitview's side of wx-utils' Customize Keyboard Shortcuts dialog. For now it holds the
//! system-wide hotkeys, the only keys Gitview keeps in its settings.

use wx_utils::shortcuts::{KeyChord, ShortcutModel, TabScope};
use wxdragon::prelude::*;

use crate::{config::Shortcuts, ui::hotkeys::GlobalAction};

/// The shortcuts in the settings, as the dialog edits them.
#[derive(Clone)]
struct Model(Shortcuts);

impl ShortcutModel for Model {
	type Action = GlobalAction;

	fn tabs(&self) -> Vec<String> {
		vec!["Global".to_owned()]
	}

	fn tab_scope(&self, _tab: usize) -> TabScope {
		TabScope::Global
	}

	fn actions(&self, _tab: usize) -> Vec<GlobalAction> {
		GlobalAction::ALL.to_vec()
	}

	fn action_name(&self, action: GlobalAction) -> String {
		action.name().to_owned()
	}

	fn chord(&self, _tab: usize, action: GlobalAction) -> Option<KeyChord> {
		action.chord(&self.0)
	}

	fn set_chord(&mut self, _tab: usize, action: GlobalAction, chord: Option<KeyChord>) {
		let chord = chord.map_or_else(|| "none".to_owned(), |chord| chord.to_shortcut_string());
		self.0.global.insert(action.id().to_owned(), chord);
	}

	fn reset_action(&mut self, _tab: usize, action: GlobalAction) {
		self.0.global.remove(action.id());
	}

	fn reset_all(&mut self, _tab: usize) {
		self.0.global.clear();
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

	use super::Model;
	use crate::{config::Shortcuts, ui::hotkeys::GlobalAction};

	#[test]
	fn edits_survive_as_settings_and_reset_goes_back_to_the_default() {
		let mut model = Model(Shortcuts::default());
		let action = GlobalAction::ShowHide;
		model.set_chord(0, action, Some(KeyChord::new(true, false, true, "F12")));
		assert_eq!(model.0.global.get("show_hide").map(String::as_str), Some("Ctrl+Shift+F12"));
		model.set_chord(0, action, None);
		assert_eq!(model.chord(0, action), None, "clearing a key keeps it unbound rather than defaulted");
		model.reset_action(0, action);
		assert_eq!(model.chord(0, action), Some(action.default_chord()));
	}
}
