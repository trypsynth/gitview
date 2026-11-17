use std::path::Path;

use logkeeper::Level;
use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding, show_error};
#[cfg(target_os = "windows")]
use wxdragon::accessible::AccRole;
use wxdragon::prelude::*;

use crate::{config::Lists, ui::view::View};

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
	pub lists: Lists,
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
	// The order is changed in the main window, with Move view up and down in its Views list.
	let label = StaticText::builder(&views_page).with_label("&Views to show:").build();
	let list = CheckListBox::builder(&views_page).with_size(LIST_SIZE).build();
	let mut views = settings.views.clone();
	fill(list, &views);
	let (ok_button, cancel_button) = build_ok_cancel_buttons(&dialog, "OK");
	let page = BoxSizer::builder(Orientation::Vertical).build();
	page.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	page.add(&list, 1, SizerFlag::Expand | SizerFlag::All, padding);
	views_page.set_sizer(page, true);
	notebook.add_page(&views_page, "Views", true, None);
	let (oldest_first, preserve_thread_order) = display_page(notebook, padding, settings.lists);
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
		read_checks(list, &mut views);
		if views.iter().any(|(_, shown)| *shown) {
			let level = log_level.get_selection().and_then(|index| LOG_LEVELS.get(index as usize));
			break Some(Settings {
				views: views.clone(),
				lists: Lists {
					oldest_first: oldest_first.get_value(),
					preserve_thread_order: preserve_thread_order.get_value(),
				},
				log_level: level.map_or(settings.log_level, |(level, _)| *level),
			});
		}
		show_error(&dialog, "Choose at least one view to show.", "Settings");
		list.set_focus();
	};
	dialog.destroy();
	result
}

/// The page for which way lists run, with Fedra's two checkboxes. Returns them in order.
fn display_page(notebook: Notebook, padding: i32, lists: Lists) -> (CheckBox, CheckBox) {
	let page = Panel::builder(&notebook).build();
	#[cfg(target_os = "windows")]
	page.set_accessibility_role(AccRole::PropertyPage);
	let oldest_first = CheckBox::builder(&page).with_label("Show oldest entries &first").build();
	oldest_first.set_value(lists.oldest_first);
	let preserve_thread_order = CheckBox::builder(&page).with_label("Always preserve thread &order").build();
	preserve_thread_order.set_value(lists.preserve_thread_order);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&oldest_first, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&preserve_thread_order, 0, SizerFlag::All, padding);
	page.set_sizer(content, true);
	notebook.add_page(&page, "Display", false, None);
	(oldest_first, preserve_thread_order)
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

fn fill(list: CheckListBox, views: &[(View, bool)]) {
	for (index, (view, shown)) in views.iter().enumerate() {
		list.append(view.title());
		list.check(u32::try_from(index).unwrap_or_default(), *shown);
	}
	list.set_selection(0, true);
}

/// Copies the list's checkmarks into `views`, which only learns of them when asked.
fn read_checks(list: CheckListBox, views: &mut [(View, bool)]) {
	for (index, (_, shown)) in views.iter_mut().enumerate() {
		*shown = list.is_checked(u32::try_from(index).unwrap_or_default());
	}
}
