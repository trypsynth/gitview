//! System-wide hotkeys, which work whatever has focus. Only Windows has them.

use std::cell::RefCell;

use key_chord::KeyChord;
use wx_utils::global_hotkeys::GlobalHotkeys;

use crate::config::Shortcuts;

/// What a system-wide hotkey can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalAction {
	ShowHide,
}

impl GlobalAction {
	pub const ALL: [Self; 1] = [Self::ShowHide];

	/// What the settings file calls it.
	pub const fn id(self) -> &'static str {
		match self {
			Self::ShowHide => "show_hide",
		}
	}

	/// What the Keyboard Shortcuts dialog calls it.
	pub const fn name(self) -> &'static str {
		match self {
			Self::ShowHide => "Show or hide Gitview",
		}
	}

	/// Fedra's own is Ctrl+Alt+F, so Gitview's sits beside it.
	pub fn default_chord(self) -> KeyChord {
		match self {
			Self::ShowHide => KeyChord::new(true, true, false, "G"),
		}
	}

	/// The key `shortcuts` binds this to, or the default when the user hasn't changed it.
	pub fn chord(self, shortcuts: &Shortcuts) -> Option<KeyChord> {
		shortcuts.global.get(self.id()).map_or_else(|| Some(self.default_chord()), |chord| KeyChord::parse(chord))
	}
}

/// The hotkeys registered now, if any.
#[derive(Default)]
pub struct Hotkeys(RefCell<Option<GlobalHotkeys>>);

impl Hotkeys {
	/// Registers every bound global action, replacing whatever was registered before, and runs
	/// `on_hotkey` on the hotkey thread when one is pressed. Returns the keys another program
	/// already holds, so the user can be told rather than left with one that does nothing.
	pub fn register(&self, shortcuts: &Shortcuts, on_hotkey: impl Fn(GlobalAction) + Send + 'static) -> Vec<KeyChord> {
		// A key this process still holds can't be registered a second time.
		*self.0.borrow_mut() = None;
		let bindings: Vec<_> =
			GlobalAction::ALL.into_iter().filter_map(|action| Some((action.chord(shortcuts)?, action))).collect();
		if bindings.is_empty() || !cfg!(windows) {
			return Vec::new();
		}
		let (running, failed) = GlobalHotkeys::register(bindings, on_hotkey);
		*self.0.borrow_mut() = running;
		failed
	}

	/// Lets the hotkeys go as Gitview closes, without waiting on their thread, which can hang the
	/// window as it closes. Windows releases them when the process ends.
	pub fn abandon(&self) {
		if let Some(hotkeys) = self.0.borrow_mut().take() {
			std::mem::forget(hotkeys);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::GlobalAction;
	use crate::config::Shortcuts;

	#[test]
	fn a_changed_key_replaces_the_default_and_none_unbinds_it() {
		let mut shortcuts = Shortcuts::default();
		let chord =
			|shortcuts: &Shortcuts| GlobalAction::ShowHide.chord(shortcuts).map(|chord| chord.to_shortcut_string());
		assert_eq!(chord(&shortcuts).as_deref(), Some("Ctrl+Alt+G"));
		shortcuts.global.insert("show_hide".to_owned(), "Ctrl+Shift+F12".to_owned());
		assert_eq!(chord(&shortcuts).as_deref(), Some("Ctrl+Shift+F12"));
		shortcuts.global.insert("show_hide".to_owned(), "none".to_owned());
		assert_eq!(chord(&shortcuts), None);
	}
}
