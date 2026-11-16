use std::{
	cell::RefCell,
	rc::Rc,
	sync::Arc,
	time::{Duration, Instant},
};

use gitview_core::{
	Client, Error,
	models::{CloseReason, Issue, IssueState, Notification, Repository},
};
use key_chord::KeyChord;
use quiet_list::{QuietList, Select};
use wx_utils::{
	post_to_ui, prompt_text, show_error,
	tasks::{Queue, background_blocking, sleep, spawn_for, spawn_local},
};
use wxdragon::{
	clipboard::Clipboard,
	prelude::*,
	timer::Timer,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

#[cfg(not(target_os = "macos"))]
use super::tray::Tray;
use super::{
	actions::{self, Action, Marks, Selected},
	dialogs::{self, Listing},
	hotkeys::{GlobalAction, Hotkeys},
	live,
	sectioned::SectionedLists,
	text::comment_count,
	view::{self, View},
	visibility::Visibility,
};
use crate::{config, token, ui::notice};

const ID_REFRESH: i32 = ID_HIGHEST + 2;
const ID_SIGN_OUT: i32 = ID_HIGHEST + 3;
const ID_PROFILE: i32 = ID_HIGHEST + 4;
const ID_SETTINGS: i32 = ID_HIGHEST + 5;
#[cfg(windows)]
const ID_SHORTCUTS: i32 = ID_HIGHEST + 6;
const ID_FILTER_FIRST: i32 = ID_HIGHEST + 10;
const ACTIONS_MENU: usize = 1;
const FILTER_MENU: usize = 2;
const WINDOW_SIZE: Size = Size { width: 800, height: 600 };
/// How often the view on screen is fetched again. GitHub asks clients not to poll the
/// notifications endpoint more than once a minute.
const POLL_INTERVAL_MS: i32 = 60_000;
/// How long a close or reopen is laid over search results, which usually catch up in seconds.
const SEARCH_CATCH_UP: Duration = Duration::from_secs(120);
/// How many dialogs Exit closes before giving up on closing them one by one.
#[cfg(not(target_os = "macos"))]
const EXIT_TRIES: u32 = 20;
/// How long Exit lets a dialog take to close before asking the next.
#[cfg(not(target_os = "macos"))]
const EXIT_WAIT: Duration = Duration::from_millis(100);
/// The key of the row a list shows in place of items, such as "No notifications.". It is always
/// the only row, so it cannot be mistaken for an item's key.
const MESSAGE_KEY: &str = "";

enum Items {
	Notifications(Vec<Notification>),
	Repositories(Vec<Repository>),
	Issues(Vec<Issue>),
}

impl Items {
	fn labels(&self) -> Vec<String> {
		match self {
			Self::Notifications(items) => items.iter().map(notification_label).collect(),
			Self::Repositories(items) => items.iter().map(repository_label).collect(),
			Self::Issues(items) => items.iter().map(issue_label).collect(),
		}
	}

	/// What tells each item apart across fetches, so the cursor can stay on an item that moved.
	fn keys(&self) -> Vec<&str> {
		match self {
			Self::Notifications(items) => items.iter().map(|item| item.id.as_str()).collect(),
			Self::Repositories(items) => items.iter().map(|item| item.full_name.as_str()).collect(),
			Self::Issues(items) => items.iter().map(|item| item.html_url.as_str()).collect(),
		}
	}
}

/// What a view holds once it has been fetched. The labels are kept so a poll that changed
/// nothing leaves the list and the Actions menu alone.
struct Loaded {
	items: Items,
	labels: Vec<String>,
}

struct State {
	client: Option<Arc<Client>>,
	/// Every view in the order chosen in Settings, with whether the Views list shows it.
	arrangement: Vec<(View, bool)>,
	view: View,
	loaded: [Option<Loaded>; View::COUNT],
	unread_only: bool,
	issue_filter: IssueState,
	/// Set while the sign-in dialog is up, so the five views in flight cannot each raise
	/// one of their own when the token turns out to be dead.
	signing_in: bool,
	/// Issues closed or reopened lately, laid over what the search-backed views fetch.
	issue_changes: Vec<IssueChange>,
	/// What to say once the view on screen has been fetched again. Said any sooner, the list
	/// changing under the cursor would talk over it.
	announcement: Option<String>,
}

/// An issue Gitview closed or reopened. Assigned and Review requested come from GitHub's search,
/// which takes a while to catch up, so for a time they are corrected by hand.
struct IssueChange {
	repo: String,
	number: u64,
	open: bool,
	made: Instant,
}

impl IssueChange {
	/// Brings `issues` up to date with `changes`, dropping any that no longer match `filter`.
	fn apply(changes: &[Self], issues: &mut Vec<Issue>, filter: IssueState) {
		for change in changes {
			let state = if change.open { "open" } else { "closed" };
			let matching = issues
				.iter_mut()
				.filter(|issue| issue.number == change.number && issue.repository() == Some(change.repo.as_str()));
			for issue in matching {
				state.clone_into(&mut issue.state);
			}
		}
		issues.retain(|issue| match filter {
			IssueState::Open => issue.is_open(),
			IssueState::Closed => !issue.is_open(),
			IssueState::All => true,
		});
	}
}

impl Default for State {
	fn default() -> Self {
		Self {
			client: None,
			arrangement: view::arrange(&[], &[]),
			view: View::Notifications,
			loaded: [const { None }; View::COUNT],
			unread_only: true,
			issue_filter: IssueState::Open,
			signing_in: false,
			issue_changes: Vec::new(),
			announcement: None,
		}
	}
}

thread_local! {
	/// The main window, for hotkeys and other copies of Gitview. They arrive on background
	/// threads, which can't hold the window themselves.
	static OPEN_WINDOW: RefCell<Option<MainWindow>> = const { RefCell::new(None) };
}

/// Runs `act` on the main window, for what arrives from another thread: a hotkey, or another copy
/// of Gitview.
fn with_open_window(act: impl FnOnce(&MainWindow)) {
	if let Some(window) = OPEN_WINDOW.with_borrow(Clone::clone) {
		act(&window);
	}
}

#[derive(Clone)]
pub struct MainWindow {
	frame: Frame,
	/// The Views list and every view's list of items, with only the view on screen shown. A
	/// list's spoken name is fixed when it is made, so each view has one of its own.
	sections: SectionedLists<View>,
	/// A hidden label that speaks through the screen reader, to confirm actions that change
	/// nothing on screen.
	live_region: StaticText,
	poll_timer: Rc<Timer<Frame>>,
	visibility: Rc<Visibility>,
	#[cfg(not(target_os = "macos"))]
	tray: Rc<Tray>,
	hotkeys: Rc<Hotkeys>,
	/// Every change Gitview makes on GitHub goes through here, one at a time, so changes arrive
	/// in the order the user made them, such as a star and the unstar after it.
	writes: Queue,
	state: Rc<RefCell<State>>,
}

impl MainWindow {
	/// Opens the main window. `door` is where copies of Gitview started later ring, which brings
	/// this one forward instead.
	pub fn open(door: Option<doorbell::Door>) {
		let frame = Frame::builder().with_title("Gitview").with_size(WINDOW_SIZE).build();
		build_menu_bar(&frame);
		let panel = Panel::builder(&frame).build();
		let settings = config::load().views;
		let arrangement = view::arrange(&settings.order, &settings.hidden);
		let first = shown(&arrangement)[0];
		// Views, then the items in the one chosen; opening an item shows it in a dialog of its own.
		let sections = SectionedLists::new(&panel, "Views", &View::ALL.map(|view| (view, view.title())));
		// Made after the lists, so no screen reader takes it for one's label.
		let live_region = live::live_region(&panel);
		panel.set_sizer(sections.sizer(2), true);
		let state = State { arrangement, view: first, ..State::default() };
		let visibility = Rc::new(Visibility::new(frame));
		#[cfg(not(target_os = "macos"))]
		let tray = {
			let (toggling, bringing) = (Rc::clone(&visibility), Rc::clone(&visibility));
			Rc::new(Tray::install(move || toggling.show_or_hide(), move || bringing.come_forward()))
		};
		let window = Self {
			frame,
			sections,
			live_region,
			poll_timer: Rc::new(Timer::new(&frame)),
			visibility,
			#[cfg(not(target_os = "macos"))]
			tray,
			hotkeys: Rc::default(),
			writes: Queue::new(),
			state: Rc::new(RefCell::new(state)),
		};
		window.fill_views();
		window.bind_events();
		window.update_filter_menu();
		window.update_actions_menu();
		frame.show(true);
		window.sections.focus_sections();
		OPEN_WINDOW.set(Some(window.clone()));
		window.register_hotkeys(&config::load().shortcuts);
		if let Some(door) = door {
			// Gitview opens no files, so whatever another copy was started with, it means "come
			// forward".
			door.listen(|_| {
				post_to_ui(|| {
					tracing::info!("another copy was started, so coming forward");
					with_open_window(|window| window.visibility.come_forward());
				});
			});
		}
		spawn_for(&frame, async move {
			match background_blocking(token::load).await {
				Some(token) => window.connect(token),
				None => window.sign_in(),
			}
		});
	}

	fn bind_events(&self) {
		let window = self.clone();
		self.frame.on_close(move |event| {
			window.hotkeys.abandon();
			// The tray icon would otherwise keep Gitview running once the window is gone.
			#[cfg(not(target_os = "macos"))]
			window.tray.remove();
			event.skip(true);
		});
		#[cfg(not(target_os = "macos"))]
		{
			let window = self.clone();
			self.tray.on_exit(move || window.exit());
		}
		let window = self.clone();
		self.frame.on_menu_selected(move |event| match event.get_id() {
			ID_REFRESH => window.fetch_all(),
			ID_PROFILE => window.edit_profile(),
			ID_SETTINGS => window.open_settings(),
			#[cfg(windows)]
			ID_SHORTCUTS => window.edit_shortcuts(),
			ID_SIGN_OUT => window.sign_out(),
			ID_EXIT => window.frame.close(false),
			id if (ID_FILTER_FIRST..ID_FILTER_FIRST + 3).contains(&id) => window.set_filter(id - ID_FILTER_FIRST),
			// The context menu's choices arrive here too, passed up from the list.
			id => match Action::from_id(id) {
				Some(action) => window.perform(action),
				None => event.skip(true),
			},
		});
		let window = self.clone();
		self.sections.on_section_changed(move |view| window.show_view(view));
		for (_, list) in self.sections.lists() {
			let window = self.clone();
			list.on_activate(move || window.perform(Action::Open));
			let window = self.clone();
			list.on_selection_changed(move || window.update_actions_menu());
			let window = self.clone();
			list.on_context_menu(move || window.show_context_menu());
		}
		let window = self.clone();
		self.poll_timer.on_tick(move |_| window.fetch_all());
	}

	/// Closes Gitview from the tray, where a dialog may still be open. Closing the window under a
	/// dialog's own event loop trips wxWidgets, so each dialog is closed first, innermost out, as
	/// Escape would close it.
	#[cfg(not(target_os = "macos"))]
	fn exit(&self) {
		let window = self.clone();
		spawn_local(async move {
			for _ in 0..EXIT_TRIES {
				if !window.visibility.close_dialog() {
					window.frame.close(true);
					return;
				}
				// The dialog's event loop ends, and the code that showed it finishes, before the
				// next one is asked.
				sleep(EXIT_WAIT).await;
			}
			// A dialog that won't close, such as a question with no Cancel, can't keep Gitview
			// running after the user asked it to stop.
			tracing::warn!("a dialog wouldn't close, so exiting without closing the window");
			window.tray.remove();
			std::process::exit(0);
		})
		.detach();
	}

	/// Registers the global hotkeys in `shortcuts`, and says which keys another program has taken.
	fn register_hotkeys(&self, shortcuts: &config::Shortcuts) {
		let taken = self.hotkeys.register(shortcuts, |action| {
			post_to_ui(move || with_open_window(|window| window.run_global(action)));
		});
		for action in GlobalAction::ALL {
			if let Some(chord) = action.chord(shortcuts).filter(|chord| !taken.contains(chord)) {
				tracing::info!(action = action.id(), key = %chord.to_shortcut_string(), "hotkey registered");
			}
		}
		if !taken.is_empty() {
			let keys: Vec<String> = taken.iter().map(KeyChord::to_shortcut_string).collect();
			notice::warning(
				&self.frame,
				format!(
					"Another program already uses {}, so Gitview can't. You can choose another key in Keyboard \
					 Shortcuts, in the File menu.",
					keys.join(" and ")
				),
				"Hotkey Not Available",
			);
		}
	}

	/// Lets the user change the global hotkeys, then saves and registers whatever they chose.
	#[cfg(windows)]
	fn edit_shortcuts(&self) {
		let mut config = config::load();
		let Some(shortcuts) = dialogs::show_shortcuts_dialog(&self.frame, &config.shortcuts) else {
			return;
		};
		if shortcuts == config.shortcuts {
			return;
		}
		config.shortcuts = shortcuts;
		tracing::info!("shortcuts changed");
		if let Err(error) = config::save(&config) {
			notice::warning(
				&self.frame,
				format!("Gitview could not save your shortcuts, so they will only last until it closes. {error}"),
				"Shortcuts Not Saved",
			);
		}
		self.register_hotkeys(&config.shortcuts);
	}

	fn run_global(&self, action: GlobalAction) {
		tracing::debug!(action = action.id(), "hotkey pressed");
		match action {
			GlobalAction::ShowHide => self.visibility.toggle(),
		}
	}

	fn connect(&self, token: String) {
		tracing::info!("signed in");
		self.state.borrow_mut().client = Some(Arc::new(Client::new(token)));
		self.fetch_all();
		self.poll_timer.start(POLL_INTERVAL_MS, false);
	}

	fn sign_in(&self) {
		if self.state.borrow().signing_in {
			return;
		}
		self.state.borrow_mut().signing_in = true;
		let token = dialogs::show_sign_in_dialog(&self.frame);
		self.state.borrow_mut().signing_in = false;
		let Some(token) = token else {
			tracing::info!("sign-in cancelled, so closing");
			self.frame.close(true);
			return;
		};
		self.save_token(&token);
		self.connect(token);
	}

	fn save_token(&self, token: &str) {
		if let Err(error) = token::save(token) {
			notice::warning(
				&self.frame,
				format!("Gitview could not save your sign-in, so you will need to sign in again next time. {error}"),
				"Sign-In Not Saved",
			);
		}
	}

	fn edit_profile(&self) {
		let Some(client) = self.state.borrow().client.clone() else {
			return;
		};
		let window = self.clone();
		dialogs::edit_profile(self.frame, client, move || window.reauthorize());
	}

	/// Signs in again to pick up scopes added since the saved sign-in, then goes back to the
	/// profile. Cancelling keeps the old sign-in, which still works for everything else.
	fn reauthorize(&self) {
		if self.state.borrow().signing_in {
			return;
		}
		self.state.borrow_mut().signing_in = true;
		let token = dialogs::show_sign_in_dialog(&self.frame);
		self.state.borrow_mut().signing_in = false;
		let Some(token) = token else {
			return;
		};
		self.save_token(&token);
		self.state.borrow_mut().client = Some(Arc::new(Client::new(token)));
		self.edit_profile();
	}

	fn sign_out(&self) {
		tracing::info!("signing out");
		self.poll_timer.stop();
		token::delete();
		{
			// What belongs to the account goes; how the window is laid out stays.
			let mut state = self.state.borrow_mut();
			let arrangement = std::mem::take(&mut state.arrangement);
			*state = State { arrangement, view: state.view, ..State::default() };
		}
		for view in View::ALL {
			self.clear(view);
		}
		self.sign_in();
	}

	fn open_settings(&self) {
		let mut config = config::load();
		let current =
			dialogs::Settings { views: self.state.borrow().arrangement.clone(), log_level: config.log_level() };
		let Some(dialogs::Settings { views: arrangement, log_level }) =
			dialogs::show_settings_dialog(&self.frame, &current, &config::folder())
		else {
			return;
		};
		if log_level != current.log_level {
			logkeeper::set_level(log_level);
			tracing::info!(level = %log_level, "log level changed");
			config.log_level = Some(log_level.to_string());
		}
		config.views = config::Views {
			order: arrangement.iter().map(|(view, _)| view.id().to_owned()).collect(),
			hidden: arrangement.iter().filter(|(_, shown)| !shown).map(|(view, _)| view.id().to_owned()).collect(),
		};
		if arrangement != current.views {
			tracing::info!(order = ?config.views.order, hidden = ?config.views.hidden, "views changed");
		}
		if let Err(error) = config::save(&config) {
			notice::warning(
				&self.frame,
				format!("Gitview could not save your settings, so they will only last until it closes. {error}"),
				"Settings Not Saved",
			);
		}
		self.state.borrow_mut().arrangement = arrangement;
		self.fill_views();
	}

	/// Lists the shown views in their chosen order. The view on screen stays when it is still
	/// shown; otherwise the first shown view takes its place.
	fn fill_views(&self) {
		let shown = shown(&self.state.borrow().arrangement);
		self.sections.set_order(&shown);
	}

	/// Catches up with a view the user just moved to, now on screen. Its list kept its items and
	/// its place while hidden, since every fetch fills it.
	fn show_view(&self, view: View) {
		self.state.borrow_mut().view = view;
		self.update_filter_menu();
		self.update_actions_menu();
		// Startup loads every view, so this only happens when that fetch is still in flight or
		// failed.
		if self.state.borrow().loaded[view.index()].is_none() {
			self.clear(view);
			self.fetch(view);
		}
	}

	fn list(&self, view: View) -> &QuietList<String> {
		self.sections.list(view)
	}

	fn current_list(&self) -> &QuietList<String> {
		self.sections.current_list()
	}

	/// Empties `view`'s list, which is how a view shows that it is loading.
	fn clear(&self, view: View) {
		self.list(view).update_labelled(Vec::new(), Select::Start);
	}

	/// Shows `message` alone in `view`'s list, in place of items.
	fn show_message(&self, view: View, message: &str) {
		self.list(view).update_labelled(vec![(MESSAGE_KEY.to_owned(), message.to_owned())], Select::Start);
	}

	fn set_filter(&self, index: i32) {
		{
			let mut state = self.state.borrow_mut();
			let view = state.view;
			match view {
				View::Notifications => state.unread_only = index == 0,
				View::Repositories | View::Starred | View::Watched => return,
				View::Assigned | View::ReviewRequested => {
					state.issue_filter = match index {
						0 => IssueState::Open,
						1 => IssueState::Closed,
						_ => IssueState::All,
					};
				}
			}
			// What is held no longer matches the filter, so it cannot stand in while the
			// new fetch runs.
			state.loaded[view.index()] = None;
		}
		let view = self.state.borrow().view;
		self.clear(view);
		self.update_actions_menu();
		self.fetch(view);
	}

	/// Rebuilds the Filter menu for the selected view, since the filters differ per view.
	fn update_filter_menu(&self) {
		let Some(menu_bar) = self.frame.get_menu_bar() else {
			return;
		};
		let state = self.state.borrow();
		let filters = state.view.filters();
		let menu = Menu::builder().build();
		if filters.is_empty() {
			menu.append(ID_FILTER_FIRST, "No filters for this view", "", ItemKind::Normal);
			menu.enable_item(ID_FILTER_FIRST, false);
		} else {
			for (index, label) in filters.iter().enumerate() {
				let id = ID_FILTER_FIRST + i32::try_from(index).unwrap_or_default();
				menu.append(id, label, "Show only these items", ItemKind::Radio);
			}
			let selected = match state.view {
				View::Notifications => i32::from(!state.unread_only),
				_ => match state.issue_filter {
					IssueState::Open => 0,
					IssueState::Closed => 1,
					IssueState::All => 2,
				},
			};
			menu.check_item(ID_FILTER_FIRST + selected, true);
		}
		menu_bar.replace(FILTER_MENU, menu, "F&ilter");
	}

	fn fetch_all(&self) {
		for view in View::ALL {
			self.fetch(view);
		}
	}

	/// Asks GitHub for `view` again. The list keeps what it has until the answer arrives.
	fn fetch(&self, view: View) {
		let (Some(client), unread_only, issue_filter) = ({
			let state = self.state.borrow();
			(state.client.clone(), state.unread_only, state.issue_filter)
		}) else {
			return;
		};
		let fetched = background_blocking(move || match view {
			View::Notifications => client.notifications(!unread_only).map(Items::Notifications),
			View::Repositories => client.repositories().map(Items::Repositories),
			View::Starred => client.starred().map(Items::Repositories),
			View::Watched => client.watched().map(Items::Repositories),
			View::Assigned => client.assigned(issue_filter).map(Items::Issues),
			View::ReviewRequested => client.review_requests(issue_filter).map(Items::Issues),
		});
		let window = self.clone();
		spawn_for(&self.frame, async move { window.store(view, fetched.await) });
	}

	fn store(&self, view: View, result: Result<Items, Error>) {
		let mut items = match result {
			Ok(items) => items,
			Err(Error::Unauthorized) => {
				tracing::warn!(view = view.id(), "GitHub no longer accepts the saved sign-in");
				token::delete();
				self.sign_in();
				return;
			}
			Err(error) => {
				// A failed poll leaves whatever the view already holds on screen, and a
				// view the user is not looking at raises no error.
				let state = self.state.borrow();
				let (unloaded, current) = (state.loaded[view.index()].is_none(), state.view == view);
				drop(state);
				// The view on screen says so in a dialog, which logs it too.
				if !(unloaded && current) {
					tracing::warn!(view = view.id(), %error, "couldn't load");
				}
				if unloaded {
					self.show_message(view, "Could not load this view.");
					if current {
						notice::error(&self.frame, error, "Could Not Load GitHub Data");
					}
				}
				if current {
					self.speak_announcement();
				}
				return;
			}
		};
		if let Items::Issues(issues) = &mut items {
			let mut state = self.state.borrow_mut();
			state.issue_changes.retain(|change| change.made.elapsed() < SEARCH_CATCH_UP);
			IssueChange::apply(&state.issue_changes, issues, state.issue_filter);
		}
		let labels = items.labels();
		tracing::debug!(view = view.id(), items = labels.len(), "loaded");
		let (current, unchanged) = {
			let mut state = self.state.borrow_mut();
			let unchanged = state.loaded[view.index()]
				.as_ref()
				.is_some_and(|loaded| loaded.labels == labels && loaded.items.keys() == items.keys());
			state.loaded[view.index()] = Some(Loaded { items, labels });
			(state.view == view, unchanged)
		};
		if !unchanged {
			self.fill(view);
		}
		if current {
			self.speak_announcement();
		}
	}

	/// Says what an action did, now that the list it changed is settled.
	fn speak_announcement(&self) {
		let announcement = self.state.borrow_mut().announcement.take();
		if let Some(message) = announcement {
			self.announce(&message);
		}
	}

	/// Puts what `view` has loaded in its list, or its message when there is nothing. The cursor
	/// stays on the item it was on wherever the fetch moved it, and the screen reader says nothing
	/// until the user moves.
	fn fill(&self, view: View) {
		let rows = {
			let state = self.state.borrow();
			let Some(loaded) = state.loaded[view.index()].as_ref() else {
				return;
			};
			if loaded.labels.is_empty() {
				vec![(MESSAGE_KEY.to_owned(), view.empty_message().to_owned())]
			} else {
				loaded.items.keys().into_iter().map(str::to_owned).zip(loaded.labels.iter().cloned()).collect()
			}
		};
		self.list(view).update_labelled(rows, Select::Keep);
		// Even with the cursor where it was, the item under it may offer other actions now, and
		// the Starred and Watching lists decide what every repository offers.
		self.update_actions_menu();
	}

	/// A copy of the selected item, or `None` while the list is empty or holds only a message.
	/// The item is found by its key rather than its row, so an action can never land on a
	/// neighbour.
	fn selected(&self) -> Option<Selected> {
		let key = self.current_list().selected()?;
		let state = self.state.borrow();
		let items = &state.loaded[state.view.index()].as_ref()?.items;
		let index = items.keys().iter().position(|candidate| *candidate == key)?;
		Some(match items {
			Items::Notifications(notifications) => Selected::Notification(notifications.get(index)?.clone()),
			Items::Repositories(repositories) => Selected::Repository(repositories.get(index)?.clone()),
			Items::Issues(issues) => Selected::Issue(issues.get(index)?.clone()),
		})
	}

	/// Whether `selected` is a repository in the Starred and Watching views' lists.
	fn marks(&self, selected: &Selected) -> Marks {
		let Selected::Repository(repository) = selected else {
			return Marks::default();
		};
		let state = self.state.borrow();
		let listed = |view: View| {
			matches!(
				state.loaded[view.index()].as_ref().map(|loaded| &loaded.items),
				Some(Items::Repositories(repositories))
					if repositories.iter().any(|listed| listed.full_name == repository.full_name)
			)
		};
		Marks { starred: listed(View::Starred), watching: listed(View::Watched) }
	}

	/// Rebuilds the Actions menu for the selected item, so its shortcuts act on that item.
	fn update_actions_menu(&self) {
		let Some(menu_bar) = self.frame.get_menu_bar() else {
			return;
		};
		let selected = self.selected();
		let marks = selected.as_ref().map(|selected| self.marks(selected)).unwrap_or_default();
		menu_bar.replace(ACTIONS_MENU, actions::menu(selected.as_ref(), marks), "&Actions");
	}

	fn show_context_menu(&self) {
		let Some(selected) = self.selected() else {
			return;
		};
		let mut menu = actions::menu(Some(&selected), self.marks(&selected));
		self.current_list().popup_menu(&mut menu);
		menu.destroy_menu();
	}

	fn perform(&self, action: Action) {
		let (Some(client), Some(selected)) = (self.state.borrow().client.clone(), self.selected()) else {
			return;
		};
		match action {
			Action::Open => self.open_item(client, &selected),
			Action::Comment => {
				let Some((repo, number)) = selected.issue() else {
					return;
				};
				let window = self.clone();
				dialogs::post_comment(self.frame, client, repo, number, "", move |_| {
					window.state.borrow_mut().announcement = Some("Comment posted.".to_owned());
					window.fetch_all();
				});
			}
			Action::CloseCompleted | Action::CloseNotPlanned | Action::ClosePullRequest | Action::Reopen => {
				let Some((repo, number)) = selected.issue() else {
					return;
				};
				let message = match action {
					Action::CloseCompleted => "Closed as completed.",
					Action::CloseNotPlanned => "Closed as not planned.",
					Action::ClosePullRequest => "Pull request closed.",
					_ => "Reopened.",
				};
				let change =
					IssueChange { repo: repo.clone(), number, open: action == Action::Reopen, made: Instant::now() };
				self.change(message, Some(change), move || match action {
					Action::CloseCompleted => client.close_issue(&repo, number, CloseReason::Completed),
					Action::CloseNotPlanned => client.close_issue(&repo, number, CloseReason::NotPlanned),
					Action::ClosePullRequest => client.close_pull_request(&repo, number),
					_ => client.reopen(&repo, number),
				});
			}
			Action::CloseDuplicate => self.close_as_duplicate(client, &selected),
			Action::MarkRead | Action::MarkDone | Action::Unsubscribe => {
				let Selected::Notification(notification) = selected else {
					return;
				};
				let id = notification.id;
				let message = match action {
					Action::MarkRead => "Marked as read.",
					Action::MarkDone => "Marked as done.",
					_ => "Unsubscribed.",
				};
				self.change(message, None, move || match action {
					Action::MarkRead => client.mark_notification_read(&id),
					Action::MarkDone => client.mark_notification_done(&id),
					_ => client.unsubscribe(&id),
				});
			}
			Action::Issues | Action::PullRequests => {
				let Selected::Repository(repository) = selected else {
					return;
				};
				let listing = if action == Action::Issues { Listing::Issues } else { Listing::PullRequests };
				let state = self.state.borrow().issue_filter;
				dialogs::open_issues(self.frame, client, repository.full_name, state, listing);
			}
			Action::Star | Action::Unstar => {
				let Selected::Repository(repository) = selected else {
					return;
				};
				let starring = action == Action::Star;
				let message = if starring { "Starred." } else { "Unstarred." };
				self.change(message, None, move || client.set_starred(&repository.full_name, starring));
			}
			Action::Watch | Action::Unwatch => {
				let Selected::Repository(repository) = selected else {
					return;
				};
				let watching = action == Action::Watch;
				let message = if watching { "Watching." } else { "No longer watching." };
				self.change(message, None, move || client.set_watching(&repository.full_name, watching));
			}
			Action::CopyLink => {
				let window = self.clone();
				self.with_link(client, &selected, move |url| {
					if Clipboard::get().set_text(url) {
						window.announce("Link copied.");
					} else {
						notice::error(&window.frame, "Gitview couldn't copy the link.", "Could Not Copy");
					}
				});
			}
			Action::OpenInBrowser => {
				self.with_link(client, &selected, |url| {
					launch_default_browser(url, BrowserLaunchFlags::Default);
				});
			}
		}
	}

	fn open_item(&self, client: Arc<Client>, selected: &Selected) {
		match selected {
			Selected::Notification(notification) => self.open_notification(client, notification),
			Selected::Repository(repository) => {
				dialogs::open_repository(self.frame, client, repository.full_name.clone());
			}
			Selected::Issue(_) => {
				if let Some((repo, number)) = selected.issue() {
					dialogs::open_issue(self.frame, client, repo, number);
				}
			}
		}
	}

	/// Makes a change on GitHub, then fetches every view again so the lists show it, and says
	/// `message` once the view on screen has caught up. `issue_change` is set when the change closes or reopens an issue.
	fn change(
		&self,
		message: impl Into<String>,
		issue_change: Option<IssueChange>,
		job: impl FnOnce() -> Result<(), Error> + Send + 'static,
	) {
		let made = self.writes.run(job);
		let window = self.clone();
		let message = message.into();
		spawn_for(&self.frame, async move {
			match made.await {
				Ok(()) => {
					tracing::info!("{message}");
					window.state.borrow_mut().announcement = Some(message);
					if let Some(change) = issue_change {
						window.state.borrow_mut().issue_changes.push(change);
						// Search would bring the old state straight back, so the lists are fixed
						// here rather than waiting on the fetch.
						for view in [View::Assigned, View::ReviewRequested] {
							window.apply_issue_changes(view);
						}
					}
					window.fetch_all();
				}
				Err(error) => notice::error(&window.frame, error, "GitHub Did Not Make the Change"),
			}
		});
	}

	fn apply_issue_changes(&self, view: View) {
		{
			let mut state = self.state.borrow_mut();
			let State { loaded, issue_changes, issue_filter, .. } = &mut *state;
			let Some(loaded) = loaded[view.index()].as_mut() else {
				return;
			};
			let Items::Issues(issues) = &mut loaded.items else {
				return;
			};
			IssueChange::apply(issue_changes, issues, *issue_filter);
			loaded.labels = loaded.items.labels();
		}
		self.fill(view);
	}

	fn announce(&self, message: &str) {
		live_region::announce(self.live_region, message);
	}

	fn close_as_duplicate(&self, client: Arc<Client>, selected: &Selected) {
		let Some((repo, number)) = selected.issue() else {
			return;
		};
		let Some(answer) = prompt_text(
			&self.frame,
			"Duplicate of which issue? Enter its number, owner/name#number, or its link.",
			"Close as Duplicate",
		) else {
			return;
		};
		let Some((original_repo, original_number)) = parse_issue_reference(&answer, &repo) else {
			show_error(&self.frame, format!("\"{}\" doesn't name an issue.", answer.trim()), "Close as Duplicate");
			return;
		};
		let message = if original_repo == repo {
			format!("Closed as a duplicate of #{original_number}.")
		} else {
			format!("Closed as a duplicate of {original_repo}#{original_number}.")
		};
		let change = IssueChange { repo: repo.clone(), number, open: false, made: Instant::now() };
		// GitHub takes the original's id rather than its number, so it is looked up first.
		self.change(message, Some(change), move || {
			let original = client.issue(&original_repo, original_number)?;
			client.close_issue(&repo, number, CloseReason::Duplicate(original.id))
		});
	}

	/// Hands `use_link` the selected item's page on GitHub. Notifications other than issues
	/// only say where their page is once fetched.
	fn with_link(&self, client: Arc<Client>, selected: &Selected, use_link: impl FnOnce(&str) + 'static) {
		let (url, subject_url) = match selected {
			Selected::Repository(repository) => (Some(repository.html_url.clone()), None),
			Selected::Issue(issue) => (Some(issue.html_url.clone()), None),
			Selected::Notification(notification) => (notification.issue_url(), notification.subject.url.clone()),
		};
		if let Some(url) = url {
			use_link(&url);
			return;
		}
		let frame = self.frame;
		let Some(subject_url) = subject_url else {
			notice::error(&frame, "GitHub gives no link for this notification.", "No Link");
			return;
		};
		spawn_for(&frame, async move {
			match background_blocking(move || client.subject(&subject_url)).await.map(|details| details.html_url) {
				Ok(Some(url)) => use_link(&url),
				Ok(None) => notice::error(&frame, "GitHub gives no link for this notification.", "No Link"),
				Err(error) => notice::error(&frame, error, "Could Not Get the Link"),
			}
		});
	}

	/// Opens the issue thread behind a notification, or for releases and the like, what the
	/// notification points at.
	fn open_notification(&self, client: Arc<Client>, notification: &Notification) {
		let read_client = Arc::clone(&client);
		let id = notification.id.clone();
		// Nothing waits on this, and a failure costs only a notification left unread.
		self.writes.push(move || drop(read_client.mark_notification_read(&id)));
		if let Some(number) = notification.issue_number() {
			dialogs::open_issue(self.frame, client, notification.repository.full_name.clone(), number);
			return;
		}
		let meta = format!(
			"{} in {}. {}.",
			kind_label(&notification.subject.kind),
			notification.repository.full_name,
			notification.reason.replace('_', " "),
		);
		dialogs::open_subject(
			self.frame,
			client,
			notification.subject.title.clone(),
			meta,
			notification.subject.url.clone(),
		);
	}
}

/// The views the Views list shows, in order. Never empty: were every view hidden, as a hand-edited
/// config file could have it, all of them are shown instead.
fn shown(arrangement: &[(View, bool)]) -> Vec<View> {
	let shown: Vec<View> = arrangement.iter().filter(|(_, shown)| *shown).map(|(view, _)| *view).collect();
	if shown.is_empty() { arrangement.iter().map(|(view, _)| *view).collect() } else { shown }
}

/// Builds the menu bar. The Actions and Filter menus start empty, since what they hold depends
/// on the selected item and view.
fn build_menu_bar(frame: &Frame) {
	let file_menu = Menu::builder().build();
	file_menu.append(ID_REFRESH, "&Refresh\tF5", "Reload the selected view", ItemKind::Normal);
	file_menu.append_separator();
	file_menu.append(ID_SETTINGS, "&Settings...\tCtrl+,", "Choose which views to show", ItemKind::Normal);
	#[cfg(windows)]
	file_menu.append(
		ID_SHORTCUTS,
		"&Keyboard Shortcuts...",
		"Change the keys that work from anywhere",
		ItemKind::Normal,
	);
	file_menu.append(ID_PROFILE, "Edit &Profile...", "Change your public GitHub profile", ItemKind::Normal);
	file_menu.append(ID_SIGN_OUT, "Sign O&ut", "Forget your GitHub sign-in", ItemKind::Normal);
	file_menu.append(ID_EXIT, "E&xit", "Close Gitview", ItemKind::Normal);
	frame.set_menu_bar(
		MenuBar::builder()
			.append(file_menu, "&File")
			.append(Menu::builder().build(), "&Actions")
			.append(Menu::builder().build(), "F&ilter")
			.build(),
	);
}

/// Reads an issue reference: `12` or `#12` in `repo`, `owner/name#12`, or a link to it.
fn parse_issue_reference(text: &str, repo: &str) -> Option<(String, u64)> {
	let text = text.trim();
	let text = text.strip_prefix("https://github.com/").unwrap_or(text);
	let (other_repo, number) = text
		.split_once('#')
		.or_else(|| text.split_once("/issues/"))
		.or_else(|| text.split_once("/pull/"))
		.unwrap_or(("", text));
	let number = number.trim_end_matches('/').parse().ok()?;
	let other_repo = other_repo.trim();
	Some((if other_repo.is_empty() { repo.to_owned() } else { other_repo.to_owned() }, number))
}

fn notification_label(notification: &Notification) -> String {
	format!(
		"{}: {}. {}, {}",
		notification.repository.full_name,
		notification.subject.title.trim_end_matches('.'),
		kind_label(&notification.subject.kind),
		notification.reason.replace('_', " "),
	)
}

fn kind_label(kind: &str) -> &str {
	match kind {
		"PullRequest" => "Pull request",
		"CheckSuite" => "Check suite",
		"RepositoryVulnerabilityAlert" => "Security alert",
		other => other,
	}
}

fn repository_label(repository: &Repository) -> String {
	let mut label = format!(
		"{}: {}",
		repository.full_name,
		repository.description.as_deref().filter(|text| !text.is_empty()).unwrap_or("no description"),
	);
	if repository.private {
		label.push_str(", private");
	}
	if repository.fork {
		label.push_str(", fork");
	}
	label
}

fn issue_label(issue: &Issue) -> String {
	let kind = if issue.is_pull_request() { "pull request" } else { "issue" };
	format!(
		"{}: #{} {}, {kind} by {}, {}",
		issue.repository().unwrap_or("unknown"),
		issue.number,
		issue.title,
		issue.user.login,
		comment_count(issue.comments),
	)
}

#[cfg(test)]
mod tests {
	use super::parse_issue_reference;

	#[test]
	fn issue_references_name_a_repository_and_number() {
		let here = |number| Some(("me/app".to_owned(), number));
		assert_eq!(parse_issue_reference("12", "me/app"), here(12));
		assert_eq!(parse_issue_reference(" #12 ", "me/app"), here(12));
		assert_eq!(parse_issue_reference("them/lib#3", "me/app"), Some(("them/lib".to_owned(), 3)));
		assert_eq!(
			parse_issue_reference("https://github.com/them/lib/issues/3", "me/app"),
			Some(("them/lib".to_owned(), 3))
		);
		assert_eq!(
			parse_issue_reference("https://github.com/them/lib/pull/4/", "me/app"),
			Some(("them/lib".to_owned(), 4))
		);
		assert_eq!(parse_issue_reference("soon", "me/app"), None);
	}
}
