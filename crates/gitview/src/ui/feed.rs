//! Lists in a dialog that GitHub hands out a page at a time, loaded as far as the user reads, as
//! Fedra loads a timeline. A feed fills one section's list or several, the way a repository's
//! issues fill both its issue and pull request sections.

use std::{
	cell::RefCell,
	rc::{Rc, Weak},
	sync::Arc,
};

use gitview_core::{Client, Cursor, Error, Page};
use key_chord::KeyChord;
use quiet_list::{QuietList, Select};
use serde::de::DeserializeOwned;
use wx_utils::tasks::{background_blocking, spawn_for};
use wxdragon::{
	clipboard::Clipboard,
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

use super::{actions::Action, dialogs::is_plain_enter, keymap::Command, notice, sectioned::SectionedLists};
use crate::config::{self, Lists};

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
}

/// Opens rows on Enter or a double-click, and copies a row or its link, or opens the link in the
/// browser, on the keys the main window's Copy, Copy link and Open in browser have. A message row
/// has no key, so it opens nothing and has no link, but copies like any other row. What a key did
/// is said through `live_region`.
pub fn on_rows<S: FeedSection>(sections: &SectionedLists<S>, live_region: StaticText, actions: RowActions<S>) {
	let actions = Rc::new(actions);
	let shortcuts = config::load().shortcuts;
	let chord = |action| Command::Item(action).chord(&shortcuts);
	let keys = Rc::new([chord(Action::Copy), chord(Action::CopyLink), chord(Action::OpenInBrowser)]);
	for (section, list) in sections.lists() {
		let opener = {
			let (list, actions) = (Rc::downgrade(list), Rc::clone(&actions));
			Rc::new(move || {
				if let Some(key) = Weak::upgrade(&list).and_then(|list| list.selected()).filter(|key| !key.is_empty()) {
					(actions.open)(section, key);
				}
			})
		};
		let open_on_activate = Rc::clone(&opener);
		list.on_activate(move || open_on_activate());
		let (reached, actions, keys) = (Rc::downgrade(list), Rc::clone(&actions), Rc::clone(&keys));
		list.on_key_down(move |event| {
			if is_plain_enter(event) {
				opener();
				return true;
			}
			let (WindowEventData::Keyboard(key), Some(list)) = (event, reached.upgrade()) else {
				return false;
			};
			let Some(code) = key.get_key_code() else {
				return false;
			};
			let pressed = |chord: &Option<KeyChord>| {
				chord
					.as_ref()
					.is_some_and(|chord| chord.matches(code, key.control_down(), key.alt_down(), key.shift_down()))
			};
			let [copy, copy_link, open_in_browser] = &*keys;
			let link = || list.selected().filter(|key| !key.is_empty()).map(|key| (actions.link)(section, &key));
			if pressed(copy) {
				if let Some(label) = list.selected_label() {
					copy_text(live_region, &label, "Copied.");
				}
			} else if pressed(copy_link) {
				match link() {
					Some(url) => copy_text(live_region, &url, "Link copied."),
					None => live_region::announce(live_region, "No link to copy."),
				}
			} else if pressed(open_in_browser) {
				match link() {
					Some(url) => {
						// Said first, before the browser takes the screen reader's attention.
						live_region::announce(live_region, "Opening in browser.");
						launch_default_browser(&url, BrowserLaunchFlags::Default);
					}
					None => live_region::announce(live_region, "No link to open."),
				}
			} else {
				return false;
			}
			true
		});
	}
}

/// Puts `text` on the clipboard, and says `done`, or that it couldn't.
fn copy_text(live_region: StaticText, text: &str, done: &str) {
	let message = if Clipboard::get().set_text(text) { done } else { "Gitview couldn't copy that." };
	live_region::announce(live_region, message);
}
