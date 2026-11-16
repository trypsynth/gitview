//! Hiding the main window to the tray and bringing it back, along with whichever dialog the user
//! was in.

#[cfg(windows)]
use std::cell::Cell;

use wxdragon::prelude::*;

/// Whether the main window is shown, and the dialog over it with it.
pub struct Visibility {
	frame: Frame,
	/// The dialog hidden along with the window, to show again when it comes back. Hiding a window
	/// leaves the dialogs it owns on screen; only minimizing takes them too.
	#[cfg(windows)]
	hidden_dialog: Cell<isize>,
}

impl Visibility {
	pub const fn new(frame: Frame) -> Self {
		Self {
			frame,
			#[cfg(windows)]
			hidden_dialog: Cell::new(0),
		}
	}

	/// Hides Gitview when it's shown, and otherwise brings it back. For the tray menu, which takes
	/// the foreground itself, so whether Gitview was in front can't be told any more.
	pub fn show_or_hide(&self) {
		if self.frame.is_shown() {
			self.hide();
		} else {
			self.come_forward();
		}
	}

	/// Brings Gitview to the front, shown and restored, onto the window the user was last in,
	/// such as an open thread or an error message.
	pub fn come_forward(&self) {
		// Found before the frame is shown, restored or raised, any of which makes the frame the
		// window last active and hides the dialog the user was in.
		#[cfg(windows)]
		let last_active = self.take_hidden_dialog().or_else(|| win32::last_active_popup(self.frame));
		self.frame.show(true);
		if self.frame.is_iconized() {
			self.frame.iconize(false);
		}
		self.frame.raise();
		#[cfg(windows)]
		if let Some(window) = last_active {
			win32::bring_forward(window);
		}
	}

	fn hide(&self) {
		#[cfg(windows)]
		if let Some(dialog) = win32::last_active_popup(self.frame).filter(|popup| *popup != win32::handle(self.frame)) {
			win32::hide(dialog);
			self.hidden_dialog.set(dialog);
		}
		self.frame.show(false);
	}

	/// Asks the dialog open over the window, if there is one, to close, as Escape would. Returns
	/// whether there was one to ask.
	pub fn close_dialog(&self) -> bool {
		#[cfg(windows)]
		{
			let open = self
				.take_hidden_dialog()
				.or_else(|| win32::last_active_popup(self.frame).filter(|popup| *popup != win32::handle(self.frame)));
			if let Some(dialog) = open {
				win32::close(dialog);
				return true;
			}
		}
		false
	}

	#[cfg(windows)]
	fn take_hidden_dialog(&self) -> Option<isize> {
		Some(self.hidden_dialog.replace(0)).filter(|dialog| *dialog != 0)
	}
}

/// The few Windows calls the rest of this needs, with window handles as plain numbers so they
/// can sit in a `Cell`.
#[cfg(windows)]
mod win32 {
	use windows::Win32::{
		Foundation::{HWND, LPARAM, WPARAM},
		UI::WindowsAndMessaging::{
			GetLastActivePopup, PostMessageW, SW_HIDE, SW_SHOW, SetForegroundWindow, ShowWindow, WM_CLOSE,
		},
	};
	use wxdragon::prelude::*;

	pub fn handle(frame: Frame) -> isize {
		frame.get_handle() as isize
	}

	/// The window `frame` or one of its dialogs was last active in; `frame` itself when no dialog
	/// is open.
	pub fn last_active_popup(frame: Frame) -> Option<isize> {
		let frame = handle(frame);
		// SAFETY: `frame` is the live main window's handle.
		(frame != 0).then(|| unsafe { GetLastActivePopup(HWND(frame as _)) }.0 as isize)
	}

	/// Asks `window` to close once its event loop gets to it, as its close button would.
	pub fn close(window: isize) {
		// SAFETY: a handle Windows gave out for one of this process's windows.
		let _ = unsafe { PostMessageW(Some(HWND(window as _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
	}

	pub fn hide(window: isize) {
		// SAFETY: a handle Windows gave out for one of this process's windows.
		let _ = unsafe { ShowWindow(HWND(window as _), SW_HIDE) };
	}

	/// Shows `window` and puts it in front. Windows refuses the foreground to a process the user
	/// isn't using, but a hotkey, the tray, and a second copy of Gitview all count as using it.
	pub fn bring_forward(window: isize) {
		// SAFETY: a handle Windows gave out for one of this process's windows.
		unsafe {
			let _ = ShowWindow(HWND(window as _), SW_SHOW);
			let _ = SetForegroundWindow(HWND(window as _));
		}
	}
}
