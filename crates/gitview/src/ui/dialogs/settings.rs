use std::{
	cell::{Cell, RefCell},
	path::Path,
	rc::Rc,
};

use logkeeper::Level;
use wx_utils::{add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding, show_error};
#[cfg(target_os = "windows")]
use wxdragon::accessible::AccRole;
use wxdragon::prelude::*;

use crate::{
	config::{Channel, Lists, Updates},
	template::{self, Kind, Templates},
	ui::view::View,
};

const LIST_SIZE: Size = Size { width: 260, height: 180 };
const TEMPLATE_SIZE: Size = Size { width: 420, height: 120 };
/// Where the Templates page is among the pages.
const TEMPLATES_PAGE: usize = 2;
/// The log levels, quietest first, with what the Log level choice calls them.
const LOG_LEVELS: [(Level, &str); 6] = [
	(Level::Off, "Off"),
	(Level::Error, "Errors"),
	(Level::Warn, "Warnings"),
	(Level::Info, "Information"),
	(Level::Debug, "Debug"),
	(Level::Trace, "Trace"),
];

/// The update channels, with what the Update channel choice calls them.
const CHANNELS: [(Channel, &str); 2] = [(Channel::Stable, "Releases"), (Channel::Dev, "Development builds")];

/// What the Settings dialog changes.
#[derive(Clone)]
pub struct Settings {
	/// Every view in order, with whether each is shown.
	pub views: Vec<(View, bool)>,
	pub lists: Lists,
	pub templates: Templates,
	pub updates: Updates,
	pub log_level: Level,
}

/// Shows `settings` and returns them as changed, or `None` when the user cancels. `log_folder`
/// is where the log is, which the Advanced page says.
pub fn show_settings_dialog(parent: &dyn WxWidget, settings: &Settings, log_folder: &Path) -> Option<Settings> {
	let dialog = Dialog::builder(parent, "Settings").build();
	let padding = dialog_padding(&dialog);
	let notebook = Notebook::builder(&dialog).build();
	let views_page = Panel::builder(&notebook).build();
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
	add_page(notebook, views_page, "Views", true);
	let oldest_first = display_page(notebook, padding, settings.lists);
	let templates = TemplatesPage::new(notebook, padding, &settings.templates);
	let advanced = AdvancedPage::new(notebook, padding, settings, log_folder);
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
		if !views.iter().any(|(_, shown)| *shown) {
			show_error(&dialog, "Choose at least one view to show.", "Settings");
			list.set_focus();
			continue;
		}
		let edited = match templates.read() {
			Ok(edited) => edited,
			Err(mistake) => {
				notebook.set_selection(TEMPLATES_PAGE);
				show_error(&dialog, mistake, "Settings");
				templates.text.set_focus();
				continue;
			}
		};
		let level = advanced.log_level.get_selection().and_then(|index| LOG_LEVELS.get(index as usize));
		break Some(Settings {
			views: views.clone(),
			lists: Lists { oldest_first: oldest_first.get_value() },
			templates: edited,
			updates: advanced.updates(settings.updates),
			log_level: level.map_or(settings.log_level, |(level, _)| *level),
		});
	};
	dialog.destroy();
	result
}

/// The page for which way lists run. Returns its checkbox.
fn display_page(notebook: Notebook, padding: i32, lists: Lists) -> CheckBox {
	let page = Panel::builder(&notebook).build();
	let oldest_first = CheckBox::builder(&page).with_label("Show oldest entries &first").build();
	oldest_first.set_value(lists.oldest_first);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&oldest_first, 0, SizerFlag::All, padding);
	page.set_sizer(content, true);
	add_page(notebook, page, "Display", false);
	oldest_first
}

/// The page for the templates rows are shown with: which kind of row, its template, and a way
/// back to the default. The README says what each template can use.
struct TemplatesPage {
	kinds: Choice,
	text: TextCtrl,
	/// Each kind's template as edited so far, in the order of [`Kind::ALL`].
	edited: Rc<RefCell<Vec<String>>>,
	/// Which kind the text shows.
	current: Rc<Cell<usize>>,
}

impl TemplatesPage {
	fn new(notebook: Notebook, padding: i32, templates: &Templates) -> Self {
		let page = Panel::builder(&notebook).build();
		let kinds_label = StaticText::builder(&page).with_label("&Template for:").build();
		let kinds =
			Choice::builder(&page).with_choices(Kind::ALL.iter().map(|kind| kind.title().to_owned()).collect()).build();
		kinds.set_selection(0);
		let edited: Vec<String> = Kind::ALL.iter().map(|kind| templates.get(*kind).to_owned()).collect();
		let text_label = StaticText::builder(&page).with_label("T&emplate:").build();
		let text = TextCtrl::builder(&page)
			.with_style(TextCtrlStyle::MultiLine)
			.with_size(TEMPLATE_SIZE)
			.with_value(&edited[0])
			.build();
		let reset = Button::builder(&page).with_label("&Reset to default").build();
		let row = BoxSizer::builder(Orientation::Horizontal).build();
		row.add(&kinds_label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, padding);
		row.add(&kinds, 1, SizerFlag::Expand, 0);
		let content = BoxSizer::builder(Orientation::Vertical).build();
		content.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::All, padding);
		content.add(&text_label, 0, SizerFlag::Left | SizerFlag::Right, padding);
		content.add(&text, 1, SizerFlag::Expand | SizerFlag::All, padding);
		content.add(&reset, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, padding);
		page.set_sizer(content, true);
		add_page(notebook, page, "Templates", false);
		let templates_page =
			Self { kinds, text, edited: Rc::new(RefCell::new(edited)), current: Rc::new(Cell::new(0)) };
		let (edited, current) = (Rc::clone(&templates_page.edited), Rc::clone(&templates_page.current));
		kinds.on_selection_changed(move |_| {
			let Some(index) = kinds.get_selection().map(|index| index as usize) else {
				return;
			};
			let mut edited = edited.borrow_mut();
			edited[current.get()] = text.get_value();
			current.set(index);
			text.set_value(&edited[index]);
		});
		let current = Rc::clone(&templates_page.current);
		reset.on_click(move |_| {
			text.set_value(Kind::ALL[current.get()].default_template());
			text.set_focus();
		});
		templates_page
	}

	/// The templates as edited, or what's wrong with the first that can't be read, which the page
	/// is turned to.
	fn read(&self) -> Result<Templates, String> {
		let mut edited = self.edited.borrow_mut();
		edited[self.current.get()] = self.text.get_value();
		let mut templates = Templates::default();
		for (index, (kind, text)) in Kind::ALL.iter().zip(edited.iter()).enumerate() {
			if let Some(mistake) = template::mistake(text) {
				self.current.set(index);
				self.kinds.set_selection(u32::try_from(index).unwrap_or_default());
				self.text.set_value(text);
				return Err(format!("The template for {} has a mistake: {mistake}", kind.title().to_lowercase()));
			}
			templates.set(*kind, text);
		}
		Ok(templates)
	}
}

/// Adds `page` to the notebook as `title`, named so a screen reader says which page it has
/// switched to, rather than only that it's a property page.
fn add_page(notebook: Notebook, page: Panel, title: &str, selected: bool) {
	page.set_accessibility_label(title);
	#[cfg(target_os = "windows")]
	page.set_accessibility_role(AccRole::PropertyPage);
	notebook.add_page(&page, title, selected, None);
}

/// The page for what most people never change: updates, how much goes in the log, and where it
/// is.
struct AdvancedPage {
	channel: Choice,
	check_on_startup: CheckBox,
	log_level: Choice,
}

impl AdvancedPage {
	fn new(notebook: Notebook, padding: i32, settings: &Settings, log_folder: &Path) -> Self {
		let page = Panel::builder(&notebook).build();
		let channel_label = StaticText::builder(&page).with_label("&Update channel:").build();
		let channel = Choice::builder(&page).build();
		for (_, name) in CHANNELS {
			channel.append(name);
		}
		let chosen = CHANNELS.iter().position(|(channel, _)| *channel == settings.updates.channel).unwrap_or_default();
		channel.set_selection(u32::try_from(chosen).unwrap_or_default());
		let check_on_startup = CheckBox::builder(&page).with_label("&Check for updates when Gitview starts").build();
		check_on_startup.set_value(settings.updates.check_on_startup);
		let level_label = StaticText::builder(&page).with_label("&Log level:").build();
		let log_level = Choice::builder(&page).build();
		for (_, name) in LOG_LEVELS {
			log_level.append(name);
		}
		let selected = LOG_LEVELS.iter().position(|(level, _)| *level == settings.log_level).unwrap_or_default();
		log_level.set_selection(u32::try_from(selected).unwrap_or_default());
		// Made after the choice, so no screen reader takes it for the choice's label.
		let location = StaticText::builder(&page)
			.with_label(&format!("The log is gitview.log, in {}.", log_folder.display()))
			.build();
		let row = |label: &StaticText, choice: &Choice| {
			let row = BoxSizer::builder(Orientation::Horizontal).build();
			row.add(label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, padding);
			row.add(choice, 0, SizerFlag::empty(), 0);
			row
		};
		let content = BoxSizer::builder(Orientation::Vertical).build();
		content.add_sizer(&row(&channel_label, &channel), 0, SizerFlag::All, padding);
		content.add(&check_on_startup, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, padding);
		content.add_sizer(&row(&level_label, &log_level), 0, SizerFlag::All, padding);
		content.add(&location, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, padding);
		page.set_sizer(content, true);
		add_page(notebook, page, "Advanced", false);
		Self { channel, check_on_startup, log_level }
	}

	/// The update settings as chosen, or `current` where nothing is.
	fn updates(&self, current: Updates) -> Updates {
		let channel = self.channel.get_selection().and_then(|index| CHANNELS.get(index as usize));
		Updates {
			channel: channel.map_or(current.channel, |(channel, _)| *channel),
			check_on_startup: self.check_on_startup.get_value(),
		}
	}
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
