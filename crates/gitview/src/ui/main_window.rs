use std::{cell::RefCell, rc::Rc, sync::Arc};

use gitview_core::{
	Client, Error,
	models::{CloseReason, Issue, IssueState, Notification, Repository},
};
use wx_utils::{prompt_text, show_error, show_warning};
use wxdragon::{
	clipboard::Clipboard,
	prelude::*,
	timer::Timer,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

use super::{
	actions::{self, Action, Selected},
	dialogs::{self, Listing},
	text::comment_count,
	worker,
};
use crate::token;

const ID_REFRESH: i32 = ID_HIGHEST + 2;
const ID_SIGN_OUT: i32 = ID_HIGHEST + 3;
const ID_PROFILE: i32 = ID_HIGHEST + 4;
const ID_FILTER_FIRST: i32 = ID_HIGHEST + 10;
const ACTIONS_MENU: usize = 1;
const FILTER_MENU: usize = 2;
const WINDOW_SIZE: Size = Size { width: 800, height: 600 };
const LOADING: &str = "Loading...";
/// How often the view on screen is fetched again. GitHub asks clients not to poll the
/// notifications endpoint more than once a minute.
const POLL_INTERVAL_MS: i32 = 60_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
	Notifications,
	Repositories,
	Starred,
	Assigned,
	ReviewRequested,
}

impl View {
	const ALL: [Self; 5] =
		[Self::Notifications, Self::Repositories, Self::Starred, Self::Assigned, Self::ReviewRequested];
	const COUNT: usize = Self::ALL.len();

	const fn index(self) -> usize {
		match self {
			Self::Notifications => 0,
			Self::Repositories => 1,
			Self::Starred => 2,
			Self::Assigned => 3,
			Self::ReviewRequested => 4,
		}
	}

	const fn title(self) -> &'static str {
		match self {
			Self::Notifications => "Notifications",
			Self::Repositories => "Repositories",
			Self::Starred => "Starred",
			Self::Assigned => "Assigned to me",
			Self::ReviewRequested => "Review requested",
		}
	}

	/// The filters this view offers, in menu order. Repository lists have none.
	const fn filters(self) -> &'static [&'static str] {
		match self {
			Self::Notifications => &["&Unread", "&All"],
			Self::Repositories | Self::Starred => &[],
			Self::Assigned | Self::ReviewRequested => &["&Open", "&Closed", "&All"],
		}
	}

	const fn empty_message(self) -> &'static str {
		match self {
			Self::Notifications => "No notifications.",
			Self::Repositories => "No repositories.",
			Self::Starred => "No starred repositories.",
			Self::Assigned => "Nothing assigned to you.",
			Self::ReviewRequested => "No reviews requested from you.",
		}
	}
}

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
}

/// What a view holds once it has been fetched. The labels are kept so a poll that changed
/// nothing leaves the list alone, rather than rebuilding it under the user's cursor.
struct Loaded {
	items: Items,
	labels: Vec<String>,
}

struct State {
	client: Option<Arc<Client>>,
	view: View,
	loaded: [Option<Loaded>; View::COUNT],
	selections: [u32; View::COUNT],
	unread_only: bool,
	issue_filter: IssueState,
	/// Set while the sign-in dialog is up, so the five views in flight cannot each raise
	/// one of their own when the token turns out to be dead.
	signing_in: bool,
}

impl Default for State {
	fn default() -> Self {
		Self {
			client: None,
			view: View::Notifications,
			loaded: [const { None }; View::COUNT],
			selections: [0; View::COUNT],
			unread_only: true,
			issue_filter: IssueState::Open,
			signing_in: false,
		}
	}
}

#[derive(Clone)]
pub struct MainWindow {
	frame: Frame,
	views: ListBox,
	items: ListBox,
	items_label: StaticText,
	/// A hidden label that speaks through the screen reader, to confirm actions that change
	/// nothing on screen.
	live_region: StaticText,
	poll_timer: Rc<Timer<Frame>>,
	state: Rc<RefCell<State>>,
}

impl MainWindow {
	pub fn open() {
		let frame = Frame::builder().with_title("Gitview").with_size(WINDOW_SIZE).build();
		build_menu_bar(&frame);
		let panel = Panel::builder(&frame).build();
		let views_label = StaticText::builder(&panel).with_label("Views").build();
		let views = ListBox::builder(&panel).build();
		let items_label = StaticText::builder(&panel).with_label(View::Notifications.title()).build();
		let items = ListBox::builder(&panel).build();
		// Made after the lists, so no screen reader takes it for one's label.
		let live_region = StaticText::builder(&panel).with_label("").with_size(Size::new(0, 0)).build();
		live_region.show(false);
		for view in View::ALL {
			views.append(view.title());
		}
		views.set_selection(0, true);
		lay_out(panel, [(views_label, views), (items_label, items)]);
		let window = Self {
			frame,
			views,
			items,
			items_label,
			live_region,
			poll_timer: Rc::new(Timer::new(&frame)),
			state: Rc::default(),
		};
		window.bind_events();
		window.update_filter_menu();
		window.update_actions_menu();
		frame.show(true);
		views.set_focus();
		worker::spawn(token::load, move |token| match token {
			Some(token) => window.connect(token),
			None => window.sign_in(),
		});
	}

	fn bind_events(&self) {
		let window = self.clone();
		self.frame.on_menu_selected(move |event| match event.get_id() {
			ID_REFRESH => window.fetch_all(),
			ID_PROFILE => window.edit_profile(),
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
		self.views.on_selection_changed(move |event| {
			let view = event
				.get_selection()
				.and_then(|index| usize::try_from(index).ok())
				.and_then(|index| View::ALL.get(index).copied());
			if let Some(view) = view {
				window.show_view(view);
			}
		});
		let window = self.clone();
		self.items.on_item_double_clicked(move |_| window.perform(Action::Open));
		let window = self.clone();
		self.items.on_selection_changed(move |_| window.update_actions_menu());
		let window = self.clone();
		// ListBox has no context menu method of its own, so the event is bound directly.
		self.items.bind_internal(EventType::CONTEXT_MENU, move |_| window.show_context_menu());
		let window = self.clone();
		self.poll_timer.on_tick(move |_| window.fetch_all());
	}

	fn connect(&self, token: String) {
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
			self.frame.close(true);
			return;
		};
		self.save_token(&token);
		self.connect(token);
	}

	fn save_token(&self, token: &str) {
		if let Err(error) = token::save(token) {
			show_warning(
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
		self.poll_timer.stop();
		token::delete();
		*self.state.borrow_mut() = State::default();
		self.items.clear();
		self.sign_in();
	}

	/// Shows a view the user just moved to: its items appear from memory, and a fetch runs
	/// behind them in case anything changed.
	fn show_view(&self, view: View) {
		{
			let mut state = self.state.borrow_mut();
			let previous = state.view.index();
			state.selections[previous] = self.items.get_selection().unwrap_or(0);
			state.view = view;
		}
		self.items_label.set_label(view.title());
		self.update_filter_menu();
		let state = self.state.borrow();
		if let Some(loaded) = state.loaded[view.index()].as_ref() {
			let labels = loaded.labels.clone();
			let selection = state.selections[view.index()];
			drop(state);
			self.fill(view, &labels, selection);
		} else {
			drop(state);
			self.items.clear();
			self.items.append(LOADING);
			self.update_actions_menu();
			// Startup loads every view, so this only happens when that fetch is still in
			// flight or failed.
			self.fetch(view);
		}
	}

	fn set_filter(&self, index: i32) {
		{
			let mut state = self.state.borrow_mut();
			let view = state.view;
			match view {
				View::Notifications => state.unread_only = index == 0,
				View::Repositories | View::Starred => return,
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
			state.selections[view.index()] = 0;
		}
		self.items.clear();
		self.items.append(LOADING);
		self.update_actions_menu();
		self.fetch(self.state.borrow().view);
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
		let window = self.clone();
		worker::spawn(
			move || match view {
				View::Notifications => client.notifications(!unread_only).map(Items::Notifications),
				View::Repositories => client.repositories().map(Items::Repositories),
				View::Starred => client.starred().map(Items::Repositories),
				View::Assigned => client.assigned(issue_filter).map(Items::Issues),
				View::ReviewRequested => client.review_requests(issue_filter).map(Items::Issues),
			},
			move |result| window.store(view, result),
		);
	}

	fn store(&self, view: View, result: Result<Items, Error>) {
		let items = match result {
			Ok(items) => items,
			Err(Error::Unauthorized) => {
				token::delete();
				self.sign_in();
				return;
			}
			Err(error) => {
				// A failed poll leaves whatever the view already holds on screen, and a
				// view the user is not looking at stays quiet either way.
				let state = self.state.borrow();
				if state.loaded[view.index()].is_none() && state.view == view {
					drop(state);
					self.items.clear();
					self.items.append("Could not load this view.");
					show_error(&self.frame, error, "Could Not Load GitHub Data");
				}
				return;
			}
		};
		let labels = items.labels();
		let (current_view, unchanged, selection) = {
			let mut state = self.state.borrow_mut();
			let unchanged = state.loaded[view.index()].as_ref().is_some_and(|loaded| loaded.labels == labels);
			state.loaded[view.index()] = Some(Loaded { items, labels: labels.clone() });
			(state.view, unchanged, state.selections[view.index()])
		};
		if current_view != view || unchanged {
			return;
		}
		let selection = self.items.get_selection().unwrap_or(selection);
		self.fill(view, &labels, selection);
	}

	fn fill(&self, view: View, labels: &[String], selection: u32) {
		self.items.clear();
		for label in labels {
			self.items.append(label);
		}
		if labels.is_empty() {
			self.items.append(view.empty_message());
		}
		self.items.set_selection(selection.min(self.items.get_count() - 1), true);
		// Setting the selection in code raises no event, so the menu is brought up to date by hand.
		self.update_actions_menu();
	}

	/// A copy of the selected item, or `None` while the list holds only a message.
	fn selected(&self) -> Option<Selected> {
		let index = self.items.get_selection()? as usize;
		let state = self.state.borrow();
		Some(match &state.loaded[state.view.index()].as_ref()?.items {
			Items::Notifications(notifications) => Selected::Notification(notifications.get(index)?.clone()),
			Items::Repositories(repositories) => Selected::Repository(repositories.get(index)?.clone()),
			Items::Issues(issues) => Selected::Issue(issues.get(index)?.clone()),
		})
	}

	/// Whether `selected` is a repository in the Starred view's list.
	fn is_starred(&self, selected: &Selected) -> bool {
		let Selected::Repository(repository) = selected else {
			return false;
		};
		let state = self.state.borrow();
		matches!(
			state.loaded[View::Starred.index()].as_ref().map(|loaded| &loaded.items),
			Some(Items::Repositories(starred)) if starred.iter().any(|star| star.full_name == repository.full_name)
		)
	}

	/// Rebuilds the Actions menu for the selected item, so its shortcuts act on that item.
	fn update_actions_menu(&self) {
		let Some(menu_bar) = self.frame.get_menu_bar() else {
			return;
		};
		let selected = self.selected();
		let starred = selected.as_ref().is_some_and(|selected| self.is_starred(selected));
		menu_bar.replace(ACTIONS_MENU, actions::menu(selected.as_ref(), starred), "&Actions");
	}

	fn show_context_menu(&self) {
		let Some(selected) = self.selected() else {
			return;
		};
		let mut menu = actions::menu(Some(&selected), self.is_starred(&selected));
		self.items.popup_menu(&mut menu, None);
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
					window.announce("Comment posted.");
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
				self.change(message, move || match action {
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
				self.change(message, move || match action {
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
				self.change(message, move || client.set_starred(&repository.full_name, starring));
			}
			Action::CopyLink => {
				let window = self.clone();
				self.with_link(client, &selected, move |url| {
					if Clipboard::get().set_text(url) {
						window.announce("Link copied.");
					} else {
						show_error(&window.frame, "Gitview couldn't copy the link.", "Could Not Copy");
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
				let state = self.state.borrow().issue_filter;
				dialogs::open_issues(self.frame, client, repository.full_name.clone(), state, Listing::Both);
			}
			Selected::Issue(_) => {
				if let Some((repo, number)) = selected.issue() {
					dialogs::open_issue(self.frame, client, repo, number);
				}
			}
		}
	}

	/// Makes a change on GitHub, says `message` once it is made, then fetches every view again
	/// so the lists show it.
	fn change(&self, message: impl Into<String>, job: impl FnOnce() -> Result<(), Error> + Send + 'static) {
		let window = self.clone();
		let message = message.into();
		worker::spawn(job, move |result| match result {
			Ok(()) => {
				window.announce(&message);
				window.fetch_all();
			}
			Err(error) => show_error(&window.frame, error, "GitHub Did Not Make the Change"),
		});
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
		// GitHub takes the original's id rather than its number, so it is looked up first.
		self.change(message, move || {
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
			show_error(&frame, "GitHub gives no link for this notification.", "No Link");
			return;
		};
		worker::spawn(
			move || client.subject(&subject_url),
			move |result| match result.map(|details| details.html_url) {
				Ok(Some(url)) => use_link(&url),
				Ok(None) => show_error(&frame, "GitHub gives no link for this notification.", "No Link"),
				Err(error) => show_error(&frame, error, "Could Not Get the Link"),
			},
		);
	}

	/// Opens the issue thread behind a notification, or for releases and the like, what the
	/// notification points at.
	fn open_notification(&self, client: Arc<Client>, notification: &Notification) {
		let read_client = Arc::clone(&client);
		let id = notification.id.clone();
		worker::spawn(move || read_client.mark_notification_read(&id), |_| {});
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

/// Builds the menu bar. The Actions and Filter menus start empty, since what they hold depends
/// on the selected item and view.
fn build_menu_bar(frame: &Frame) {
	let file_menu = Menu::builder().build();
	file_menu.append(ID_REFRESH, "&Refresh\tF5", "Reload the selected view", ItemKind::Normal);
	file_menu.append_separator();
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

// Views, then the items in the selected view; opening an item shows what it says in a dialog of
// its own. Each label sits above its control so screen readers use it as that control's name.
fn lay_out(panel: Panel, lists: [(StaticText, ListBox); 2]) {
	let padding = panel.from_dip_int(wx_utils::DIALOG_PADDING);
	let sizer = BoxSizer::builder(Orientation::Horizontal).build();
	for (weight, (label, list)) in [1, 2].into_iter().zip(lists) {
		let column = BoxSizer::builder(Orientation::Vertical).build();
		column.add(&label, 0, SizerFlag::empty(), 0);
		column.add(&list, 1, SizerFlag::Expand, 0);
		sizer.add_sizer(&column, weight, SizerFlag::Expand | SizerFlag::All, padding);
	}
	panel.set_sizer(sizer, true);
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
