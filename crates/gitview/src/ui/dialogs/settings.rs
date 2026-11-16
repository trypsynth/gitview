use std::{cell::RefCell, path::Path, rc::Rc};

use logkeeper::Level;
use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding, show_error};
#[cfg(target_os = "windows")]
use wxdragon::accessible::AccRole;
use wxdragon::prelude::*;

use crate::ui::{live, view::View};

const LIST_SIZE: Size = Size { width: 260, height: 180 };
/// The log levels, quietest first, with what the Log level choice calls them.
const LOG_LEVELS: [(Level, &str); 6] = [
	(Level::Off, "Off"),
	(Level::Error, "Errors"),
	(Level::Warn, "Warnings"),
	(Level::Info, "Information"),
	(Level::Debug, "Debug"),
	(Level::Trace, "Trace"),
];

/// What the Settings dialog changes.
#[derive(Clone)]
pub struct Settings {
	/// Every view in order, with whether each is shown.
	pub views: Vec<(View, bool)>,
	pub log_level: Level,
}

/// Shows `settings` and returns them as changed, or `None` when the user cancels. `log_folder`
/// is where the log is, which the Advanced page says.
pub fn show_settings_dialog(parent: &dyn WxWidget, settings: &Settings, log_folder: &Path) -> Option<Settings> {
	let dialog = Dialog::builder(parent, "Settings").build();
	let padding = dialog_padding(&dialog);
	let notebook = Notebook::builder(&dialog).build();
	let views_page = Panel::builder(&notebook).build();
	#[cfg(target_os = "windows")]
	views_page.set_accessibility_role(AccRole::PropertyPage);
	let label = StaticText::builder(&views_page).with_label("&Views to show, in order:").build();
	let list = CheckListBox::builder(&views_page).with_size(LIST_SIZE).build();
	// Speaks where a moved view ended up. Made after the list, so no screen reader takes it for
	// the list's label.
	let live_region = live::live_region(&views_page);
	let up_button = Button::builder(&views_page).with_label("Move &up").build();
	let down_button = Button::builder(&views_page).with_label("Move &down").build();
	let views = Rc::new(RefCell::new(settings.views.clone()));
	fill(list, &views.borrow(), 0);
	let move_selected = {
		let views = Rc::clone(&views);
		move |down: bool| {
			let Some(index) = list.get_selection().map(|index| index as usize) else {
				return;
			};
			let mut views = views.borrow_mut();
			read_checks(list, &mut views);
			let target = if down { index + 1 } else { index.wrapping_sub(1) };
			if target >= views.len() {
				let edge = if down { "last" } else { "first" };
				live_region::announce(live_region, &format!("Already {edge}."));
				return;
			}
			views.swap(index, target);
			fill(list, &views, target);
			let title = views[target].0.title();
			live_region::announce(live_region, &format!("{title} moved to {} of {}.", target + 1, views.len()));
		}
	};
	let move_up = move_selected.clone();
	up_button.on_click(move |_| move_up(false));
	down_button.on_click(move |_| move_selected(true));
	let (ok_button, cancel_button) = build_ok_cancel_buttons(&dialog, "OK");
	let buttons = BoxSizer::builder(Orientation::Vertical).build();
	buttons.add(&up_button, 0, SizerFlag::Expand | SizerFlag::Bottom, padding / 2);
	buttons.add(&down_button, 0, SizerFlag::Expand, 0);
	let list_column = BoxSizer::builder(Orientation::Vertical).build();
	list_column.add(&label, 0, SizerFlag::empty(), 0);
	list_column.add(&list, 1, SizerFlag::Expand, 0);
	let page = BoxSizer::builder(Orientation::Horizontal).build();
	page.add_sizer(&list_column, 1, SizerFlag::Expand | SizerFlag::All, padding);
	page.add_sizer(&buttons, 0, SizerFlag::Top | SizerFlag::Right, padding);
	views_page.set_sizer(page, true);
	notebook.add_page(&views_page, "Views", true, None);
	let log_level = advanced_page(notebook, padding, settings.log_level, log_folder);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&notebook, 1, SizerFlag::Expand | SizerFlag::All, padding);
	add_ok_cancel_footer(content, ok_button, cancel_button);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	list.set_focus();
	// The main window needs a view to show, so OK with none checked asks again.
	let result = loop {
		if dialog.show_modal() != ID_OK {
			break None;
		}
		let mut views = views.borrow_mut();
		read_checks(list, &mut views);
		if views.iter().any(|(_, shown)| *shown) {
			let level = log_level.get_selection().and_then(|index| LOG_LEVELS.get(index as usize));
			break Some(Settings {
				views: views.clone(),
				log_level: level.map_or(settings.log_level, |(level, _)| *level),
			});
		}
		show_error(&dialog, "Choose at least one view to show.", "Settings");
		list.set_focus();
	};
	dialog.destroy();
	result
}

/// The page for what most people never change: how much goes in the log, and where it is.
/// Returns the log level choice.
fn advanced_page(notebook: Notebook, padding: i32, level: Level, log_folder: &Path) -> Choice {
	let page = Panel::builder(&notebook).build();
	#[cfg(target_os = "windows")]
	page.set_accessibility_role(AccRole::PropertyPage);
	let label = StaticText::builder(&page).with_label("&Log level:").build();
	let choice = Choice::builder(&page).build();
	for (_, name) in LOG_LEVELS {
		choice.append(name);
	}
	let selected = LOG_LEVELS.iter().position(|(choice, _)| *choice == level).unwrap_or_default();
	choice.set_selection(u32::try_from(selected).unwrap_or_default());
	// Made after the choice, so no screen reader takes it for the choice's label.
	let location =
		StaticText::builder(&page).with_label(&format!("The log is gitview.log, in {}.", log_folder.display())).build();
	let row = BoxSizer::builder(Orientation::Horizontal).build();
	row.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, padding);
	row.add(&choice, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add_sizer(&row, 0, SizerFlag::All, padding);
	content.add(&location, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, padding);
	page.set_sizer(content, true);
	notebook.add_page(&page, "Advanced", false, None);
	choice
}

fn fill(list: CheckListBox, views: &[(View, bool)], selection: usize) {
	list.clear();
	for (index, (view, shown)) in views.iter().enumerate() {
		list.append(view.title());
		list.check(u32::try_from(index).unwrap_or_default(), *shown);
	}
	list.set_selection(u32::try_from(selection).unwrap_or_default(), true);
}

/// Copies the list's checkmarks into `views`, which only learns of them when asked.
fn read_checks(list: CheckListBox, views: &mut [(View, bool)]) {
	for (index, (_, shown)) in views.iter_mut().enumerate() {
		*shown = list.is_checked(u32::try_from(index).unwrap_or_default());
	}
}
