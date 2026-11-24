//! Lists in a dialog that GitHub hands out a page at a time, loaded as far as the user reads, as
//! Fedra loads a timeline. A feed fills one section's list or several, the way a repository's
//! issues fill both its issue and pull request sections.

use std::{
	cell::RefCell,
	rc::{Rc, Weak},
	sync::Arc,
};

use gitview_core::{Client, Cursor, Error, Page};
use quiet_list::{QuietList, Select};
use serde::de::DeserializeOwned;
use wx_utils::{
	menu_label,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::{
	clipboard::Clipboard,
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

use super::{actions::Action, dialogs::is_plain_enter, keymap::Command, notice, sectioned::SectionedLists};
use crate::config::{self, Lists};

const ID_ROW_OPEN: i32 = ID_HIGHEST + 400;
const ID_ROW_COPY: i32 = ID_HIGHEST + 401;
const ID_ROW_COPY_KEY: i32 = ID_HIGHEST + 402;
const ID_ROW_COPY_LINK: i32 = ID_HIGHEST + 403;
const ID_ROW_OPEN_IN_BROWSER: i32 = ID_HIGHEST + 404;

/// The key of the row shown in place of items. It is always the only row, so it cannot be
/// mistaken for an item's key.
pub const MESSAGE_KEY: &str = "";

/// How many pages one reach for more goes through looking for something for the section that
/// asked, before settling for what it found. Issues and pull requests share pages, so a section
/// can come up empty on a page full of the other kind.
const PAGES_PER_LOAD: usize = 10;

/// Rows as a list shows them: each item's key and label.
pub type Rows = Vec<(String, String)>;

/// A section of a dialog that a feed fills.
pub trait FeedSection: Copy + Eq + 'static {
	fn title(self) -> &'static str;
	/// What the section says when it has nothing to list.
	fn empty_message(self) -> &'static str;
	/// Whether the section runs the way the lists setting says, rather than always newest first.
	fn follows_order(self) -> bool {
		true
	}
}

/// A list GitHub hands out a page at a time, and the sections it feeds.
pub struct Feed<S, T> {
	lists: Vec<(S, Rc<QuietList<String>>, Rows)>,
	next: Option<Cursor<T>>,
	/// Set while pages are on their way, so reaching the end again doesn't ask twice.
	loading: bool,
	/// Whether the lists run oldest first, which shows the rows, held newest first, reversed.
	oldest_first: bool,
	/// The row an item makes in a section, or `None` when the section doesn't list it.
	row: fn(S, &T) -> Option<(String, String)>,
	/// Goes up each time the feed [restarts](restart), so pages still on their way for what it
	/// listed before are dropped when they arrive.
	generation: u64,
}

/// A feed the dialog and its loading tasks share.
pub type Shared<S, T> = Rc<RefCell<Feed<S, T>>>;

impl<S: FeedSection, T> Feed<S, T> {
	/// A feed for the sections of `sections` that `feeds` picks.
	pub fn new(
		sections: &SectionedLists<S>,
		lists: Lists,
		feeds: impl Fn(S) -> bool,
		row: fn(S, &T) -> Option<(String, String)>,
	) -> Shared<S, T> {
		let shown = sections
			.lists()
			.filter(|(section, _)| feeds(*section))
			.map(|(section, list)| (section, Rc::clone(list), Vec::new()))
			.collect();
		Rc::new(RefCell::new(Self {
			lists: shown,
			next: None,
			loading: false,
			oldest_first: lists.oldest_first,
			row,
			generation: 0,
		}))
	}

	/// Adds `items` to the sections that list them, and says which sections gained rows.
	fn add(&mut self, items: &[T]) -> Vec<S> {
		let row = self.row;
		let mut gained = Vec::new();
		for (section, _, rows) in &mut self.lists {
			let before = rows.len();
			rows.extend(items.iter().filter_map(|item| row(*section, item)));
			if rows.len() > before {
				gained.push(*section);
			}
		}
		gained
	}

	fn empty_sections(&self) -> Vec<S> {
		self.lists.iter().filter(|(_, _, rows)| rows.is_empty()).map(|(section, _, _)| *section).collect()
	}
}

/// Puts each section's rows in its list, or its message when it has none.
///
/// The feed is let go before any list is filled: filling a list runs its selection callback,
/// and reaching a list's end from there loads more of this same feed.
fn show<S: FeedSection, T>(feed: &Shared<S, T>, select: &Select<String>) {
	let feed = feed.borrow();
	let shown: Vec<_> = feed
		.lists
		.iter()
		.map(|(section, list, rows)| {
			let mut rows = rows.clone();
			if feed.oldest_first && section.follows_order() {
				rows.reverse();
			}
			(Rc::clone(list), rows, section.empty_message())
		})
		.collect();
	drop(feed);
	for (list, rows, message) in shown {
		fill(&list, rows, message, select.clone());
	}
}

/// Says in each of the feed's lists why its first page couldn't be loaded.
fn show_failure<S: FeedSection, T>(feed: &Shared<S, T>, error: &Error) {
	let lists: Vec<_> = feed.borrow().lists.iter().map(|(section, list, _)| (*section, Rc::clone(list))).collect();
	for (section, list) in lists {
		let message = match error {
			Error::Restricted(organization) => format!(
				"{organization} only lets apps it has approved see this. An owner of {organization} can approve \
				 Gitview in its settings, under third-party access."
			),
			_ => format!("Could not load {}.", section.title().to_lowercase()),
		};
		fill(&list, Vec::new(), &message, Select::Start);
	}
}

/// Fills `feed` from its first page, then goes on for any section that page left empty.
pub fn start<S: FeedSection, T: DeserializeOwned + Send + 'static>(
	dialog: Dialog,
	client: Arc<Client>,
	feed: Shared<S, T>,
	first: impl FnOnce() -> Result<Page<T>, Error> + Send + 'static,
	failed_title: &'static str,
) {
	let generation = {
		let mut inner = feed.borrow_mut();
		inner.loading = true;
		inner.generation
	};
	let fetch = background_blocking(first);
	spawn_for(&dialog, async move {
		let fetched = fetch.await;
		if feed.borrow().generation != generation {
			return;
		}
		match fetched {
			Ok(page) => {
				let empty = {
					let mut inner = feed.borrow_mut();
					inner.add(&page.items);
					inner.next = page.next;
					inner.empty_sections()
				};
				show(&feed, &Select::Start);
				let result = read_on(&client, &feed, empty, generation).await;
				if feed.borrow().generation != generation {
					return;
				}
				feed.borrow_mut().loading = false;
				show(&feed, &Select::Keep);
				if let Err(error) = result {
					notice::error(&dialog, error, failed_title);
				}
			}
			Err(error) => {
				feed.borrow_mut().loading = false;
				show_failure(&feed, &error);
				// The list says why an organization's restrictions hide it, which needs no alert.
				if !matches!(error, Error::Restricted(_)) {
					notice::error(&dialog, error, failed_title);
				}
			}
		}
	});
}

/// Empties `feed` and fills it again from `first`, for when what it lists changes, such as the
/// branch whose commits it shows. Its lists stay empty until the first page arrives.
pub fn restart<S: FeedSection, T: DeserializeOwned + Send + 'static>(
	dialog: Dialog,
	client: Arc<Client>,
	feed: Shared<S, T>,
	first: impl FnOnce() -> Result<Page<T>, Error> + Send + 'static,
	failed_title: &'static str,
) {
	let lists: Vec<_> = {
		let mut inner = feed.borrow_mut();
		inner.generation += 1;
		inner.next = None;
		inner.loading = false;
		for (_, _, rows) in &mut inner.lists {
			rows.clear();
		}
		inner.lists.iter().map(|(_, list, _)| Rc::clone(list)).collect()
	};
	for list in lists {
		list.update_labelled(Vec::new(), Select::Start);
	}
	start(dialog, client, feed, first, failed_title);
}

/// Loads more of `feed` for `section`, whose oldest row was reached, as Fedra does at the end of
/// a timeline. The selection stays where it is, and the new rows are there when it moves on.
pub fn load_more<S: FeedSection, T: DeserializeOwned + Send + 'static>(
	dialog: Dialog,
	client: &Arc<Client>,
	feed: &Shared<S, T>,
	section: S,
) {
	let generation = {
		let mut inner = feed.borrow_mut();
		if inner.loading || inner.next.is_none() {
			return;
		}
		inner.loading = true;
		inner.generation
	};
	let (client, feed) = (Arc::clone(client), Rc::clone(feed));
	spawn_for(&dialog, async move {
		let result = read_on(&client, &feed, vec![section], generation).await;
		if feed.borrow().generation != generation {
			return;
		}
		feed.borrow_mut().loading = false;
		show(&feed, &Select::Keep);
		if let Err(error) = result {
			notice::error(&dialog, error, "Could Not Load More");
		}
	});
}

/// Fetches the pages after those `feed` has while any of `wanted` has gained nothing, up to
/// [`PAGES_PER_LOAD`], and stops at the list's end, or once the feed has restarted since
/// `generation`.
#[expect(clippy::future_not_send, reason = "it runs on the UI thread, where the feeds live")]
async fn read_on<S: FeedSection, T: DeserializeOwned + Send + 'static>(
	client: &Arc<Client>,
	feed: &Shared<S, T>,
	mut wanted: Vec<S>,
	generation: u64,
) -> Result<(), Error> {
	for _ in 0..PAGES_PER_LOAD {
		if wanted.is_empty() {
			break;
		}
		let Some(cursor) = feed.borrow().next.clone() else {
			break;
		};
		let client = Arc::clone(client);
		let page = background_blocking(move || client.next_page(&cursor)).await?;
		let mut inner = feed.borrow_mut();
		if inner.generation != generation {
			break;
		}
		let gained = inner.add(&page.items);
		inner.next = page.next;
		wanted.retain(|section| !gained.contains(section));
	}
	Ok(())
}

/// Puts `rows` in `list`, or `message` alone when there are none.
pub fn fill(list: &QuietList<String>, rows: Rows, message: &str, select: Select<String>) {
	let rows = if rows.is_empty() { vec![(MESSAGE_KEY.to_owned(), message.to_owned())] } else { rows };
	list.update_labelled(rows, select);
}

/// Runs `at_end` with a section whenever its list's oldest row is selected. Each list's own
/// callback holds it weakly, so the lists can go once the dialog closes.
pub fn on_reaching_oldest<S: FeedSection>(sections: &SectionedLists<S>, lists: Lists, at_end: &Rc<dyn Fn(S)>) {
	for (section, list) in sections.lists() {
		let lists = Lists { oldest_first: lists.oldest_first && section.follows_order() };
		let (reached, at_end) = (Rc::downgrade(list), Rc::clone(at_end));
		list.on_selection_changed(move || {
			if let Some(list) = reached.upgrade()
				&& list.selected_index().is_some_and(|index| lists.at_oldest(index, list.len()))
			{
				at_end(section);
			}
		});
	}
}

/// The link to the item a row of a section shows, by the row's key.
pub type Link<S> = Box<dyn Fn(S, &str) -> String>;

/// What a dialog's rows do.
pub struct RowActions<S> {
	/// Runs with a section and the chosen row's key, on Enter or a double-click.
	pub open: Box<dyn Fn(S, String)>,
	pub link: Link<S>,
	/// What a section's menu calls opening a row, such as "&View details" for a commit.
	pub open_label: fn(S) -> &'static str,
	/// What a section's menu calls copying a row's key on its own, in a section whose keys are
	/// worth copying, such as "Copy &SHA" for a commit.
	pub copy_key: fn(S) -> Option<&'static str>,
}

/// The section and list a row menu was last opened on.
type MenuTarget<S> = Rc<RefCell<Option<(S, Weak<QuietList<String>>)>>>;

/// What can be done with a row, from a key or from its menu.
#[derive(Clone, Copy)]
enum RowAction {
	Open,
	Copy,
	CopyKey,
	CopyLink,
	OpenInBrowser,
}

impl RowAction {
	const fn from_id(id: i32) -> Option<Self> {
		match id {
			ID_ROW_OPEN => Some(Self::Open),
			ID_ROW_COPY => Some(Self::Copy),
			ID_ROW_COPY_KEY => Some(Self::CopyKey),
			ID_ROW_COPY_LINK => Some(Self::CopyLink),
			ID_ROW_OPEN_IN_BROWSER => Some(Self::OpenInBrowser),
			_ => None,
		}
	}
}

/// A dialog's row actions, and where to say what they did.
struct RowHandler<S> {
	actions: RowActions<S>,
	live_region: StaticText,
	shortcuts: config::Shortcuts,
}

impl<S: FeedSection> RowHandler<S> {
	/// Does `action` to the selected row of `list`, which lists `section`. A message row has no
	/// key, so it opens nothing and has no link, but copies like any other row.
	fn perform(&self, section: S, list: &QuietList<String>, action: RowAction) {
		let key = list.selected().filter(|key| !key.is_empty());
		let say = |message: &str| live_region::announce(self.live_region, message);
		let link = || key.as_deref().map(|key| (self.actions.link)(section, key));
		match action {
			RowAction::Open => {
				if let Some(key) = key {
					(self.actions.open)(section, key);
				}
			}
			RowAction::Copy => {
				if let Some(label) = list.selected_label() {
					copy_text(self.live_region, &label, "Copied.");
				}
			}
			RowAction::CopyKey => {
				if let Some(key) = &key {
					copy_text(self.live_region, key, "Copied.");
				}
			}
			RowAction::CopyLink => match link() {
				Some(url) => copy_text(self.live_region, &url, "Link copied."),
				None => say("No link to copy."),
			},
			RowAction::OpenInBrowser => match link() {
				Some(url) => {
					// Said first, before the browser takes the screen reader's attention.
					say("Opening in browser.");
					launch_default_browser(&url, BrowserLaunchFlags::Default);
				}
				None => say("No link to open."),
			},
		}
	}

	/// The menu for a row of `section`, showing the keys its items have.
	fn menu(&self, section: S) -> Menu {
		let key = |action| Command::Item(action).accelerator(&self.shortcuts);
		let menu = Menu::builder().build();
		menu.append(ID_ROW_OPEN, &menu_label((self.actions.open_label)(section), "Enter"), "", ItemKind::Normal);
		menu.append_separator();
		menu.append(ID_ROW_COPY, &menu_label("&Copy", &key(Action::Copy)), "", ItemKind::Normal);
		if let Some(label) = (self.actions.copy_key)(section) {
			menu.append(ID_ROW_COPY_KEY, label, "", ItemKind::Normal);
		}
		menu.append(ID_ROW_COPY_LINK, &menu_label("Copy &link", &key(Action::CopyLink)), "", ItemKind::Normal);
		menu.append(
			ID_ROW_OPEN_IN_BROWSER,
			&menu_label("Open in &browser", &key(Action::OpenInBrowser)),
			"",
			ItemKind::Normal,
		);
		menu
	}
}

/// Opens rows on Enter or a double-click, and copies a row or its link, or opens the link in the
/// browser, on the keys the main window's Copy, Copy link and Open in browser have. The context
/// menu of a row has the same, along with anything its section adds. What happened is said
/// through `live_region`.
pub fn on_rows<S: FeedSection>(
	dialog: Dialog,
	sections: &SectionedLists<S>,
	live_region: StaticText,
	actions: RowActions<S>,
) {
	let shortcuts = config::load().shortcuts;
	let chord = |action| Command::Item(action).chord(&shortcuts);
	let keys = Rc::new([
		(chord(Action::Copy), RowAction::Copy),
		(chord(Action::CopyLink), RowAction::CopyLink),
		(chord(Action::OpenInBrowser), RowAction::OpenInBrowser),
	]);
	let rows = Rc::new(RowHandler { actions, live_region, shortcuts });
	// The section and list a menu was last opened on. Its choice reaches the dialog as a menu
	// event, which has only the item's ID.
	let menu_target: MenuTarget<S> = Rc::default();
	{
		let (rows, menu_target) = (Rc::clone(&rows), Rc::clone(&menu_target));
		dialog.bind_internal(EventType::MENU, move |event| {
			let target = menu_target.borrow().clone();
			match (RowAction::from_id(event.get_id()), target) {
				(Some(action), Some((section, list))) => {
					if let Some(list) = list.upgrade() {
						rows.perform(section, &list, action);
					}
				}
				_ => event.skip(true),
			}
		});
	}
	for (section, list) in sections.lists() {
		let (opened, activated) = (Rc::downgrade(list), Rc::clone(&rows));
		list.on_activate(move || {
			if let Some(list) = opened.upgrade() {
				activated.perform(section, &list, RowAction::Open);
			}
		});
		let (reached, pressed_on, keys) = (Rc::downgrade(list), Rc::clone(&rows), Rc::clone(&keys));
		list.on_key_down(move |event| {
			let Some(list) = reached.upgrade() else {
				return false;
			};
			if is_plain_enter(event) {
				pressed_on.perform(section, &list, RowAction::Open);
				return true;
			}
			let WindowEventData::Keyboard(key) = event else {
				return false;
			};
			let Some(code) = key.get_key_code() else {
				return false;
			};
			let pressed = keys.iter().find(|(chord, _)| {
				chord
					.as_ref()
					.is_some_and(|chord| chord.matches(code, key.control_down(), key.alt_down(), key.shift_down()))
			});
			let Some(&(_, action)) = pressed else {
				return false;
			};
			pressed_on.perform(section, &list, action);
			true
		});
		let (shown_on, menu_rows, menu_target) = (Rc::downgrade(list), Rc::clone(&rows), Rc::clone(&menu_target));
		list.on_context_menu(move || {
			let Some(list) = shown_on.upgrade() else {
				return;
			};
			// A row saying a list is empty has nothing to act on.
			if list.selected().is_none_or(|key| key.is_empty()) {
				return;
			}
			*menu_target.borrow_mut() = Some((section, Rc::downgrade(&list)));
			let mut menu = menu_rows.menu(section);
			list.popup_menu(&mut menu);
			menu.destroy_menu();
		});
	}
}

/// Puts `text` on the clipboard, and says `done`, or that it couldn't.
fn copy_text(live_region: StaticText, text: &str, done: &str) {
	let message = if Clipboard::get().set_text(text) { done } else { "Gitview couldn't copy that." };
	live_region::announce(live_region, message);
}
