use std::{cell::RefCell, rc::Rc};

use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding, show_error};
#[cfg(target_os = "windows")]
use wxdragon::accessible::AccRole;
use wxdragon::prelude::*;

use crate::ui::view::View;

const LIST_SIZE: Size = Size { width: 260, height: 180 };

/// Shows the settings and returns them as changed, or `None` when the user cancels. `views`
/// is every view in order, with whether each is shown.
pub fn show_settings_dialog(parent: &dyn WxWidget, views: &[(View, bool)]) -> Option<Vec<(View, bool)>> {
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
	let live_region = StaticText::builder(&views_page).with_label("").with_size(Size::new(0, 0)).build();
	live_region.show(false);
	let up_button = Button::builder(&views_page).with_label("Move &up").build();
	let down_button = Button::builder(&views_page).with_label("Move &down").build();
	let views = Rc::new(RefCell::new(views.to_vec()));
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
			break Some(views.clone());
		}
		show_error(&dialog, "Choose at least one view to show.", "Settings");
		list.set_focus();
	};
	dialog.destroy();
	result
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
