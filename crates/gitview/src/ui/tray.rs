//! The notification area icon. Clicking it brings Gitview forward, and its menu shows or hides
//! Gitview, or exits.

use std::{cell::RefCell, rc::Rc};

use wxdragon::prelude::*;

const ID_SHOW_HIDE: i32 = ID_HIGHEST + 300;
const ID_TRAY_EXIT: i32 = ID_HIGHEST + 301;

type Exit = Rc<RefCell<Option<Box<dyn Fn()>>>>;

pub struct Tray {
	icon: TaskBarIcon,
	/// Kept for as long as the icon shows it.
	menu: RefCell<Option<Menu>>,
	exit: Exit,
}

impl Tray {
	/// Puts the icon in the notification area. `show_hide` runs from its menu, and `come_forward`
	/// when the icon is clicked. Exit does nothing until [`on_exit`](Self::on_exit) says what it
	/// does.
	pub fn install(show_hide: impl Fn() + 'static, come_forward: impl Fn() + 'static) -> Self {
		let mut menu = Menu::builder()
			.append_item(ID_SHOW_HIDE, "&Show/Hide", "Show or hide Gitview")
			.append_separator()
			.append_item(ID_TRAY_EXIT, "E&xit", "Close Gitview")
			.build();
		let icon = TaskBarIcon::builder().build();
		icon.set_popup_menu(&mut menu);
		// Gitview has no artwork of its own yet.
		if let Some(bitmap) = ArtProvider::get_bitmap(ArtId::Information, ArtClient::Menu, Some(Size::new(16, 16))) {
			let _ = icon.set_icon(&bitmap, "Gitview");
		}
		let exit: Exit = Rc::default();
		let menu_exit = Rc::clone(&exit);
		icon.on_menu(move |event| match event.get_id() {
			ID_SHOW_HIDE => show_hide(),
			ID_TRAY_EXIT => {
				if let Some(exit) = &*menu_exit.borrow() {
					exit();
				}
			}
			_ => {}
		});
		icon.on_left_up(move |_| come_forward());
		Self { icon, menu: RefCell::new(Some(menu)), exit }
	}

	/// What the menu's Exit does. Set once the window it closes exists.
	pub fn on_exit(&self, exit: impl Fn() + 'static) {
		*self.exit.borrow_mut() = Some(Box::new(exit));
	}

	/// Takes the icon out of the notification area. A tray icon keeps the app running, so this
	/// has to happen for closing the window to end Gitview. Safe to call more than once.
	pub fn remove(&self) {
		if let Some(mut menu) = self.menu.borrow_mut().take() {
			menu.destroy_menu();
		}
		self.icon.destroy();
	}
}
