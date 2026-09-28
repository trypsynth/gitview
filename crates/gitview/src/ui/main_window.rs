use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use gitview_core::{
	Client, Error,
	models::{Issue, IssueState, Notification, Repository},
};
use wx_utils::{show_error, show_warning};
use wxdragon::{prelude::*, timer::Timer, widgets::WebView};

use super::{
	dialogs,
	text::{body_html, comment_count},
	web, worker,
};
use crate::token;

const ID_OPEN: i32 = ID_HIGHEST + 1;
const ID_REFRESH: i32 = ID_HIGHEST + 2;
const ID_SIGN_OUT: i32 = ID_HIGHEST + 3;
const ID_FILTER_FIRST: i32 = ID_HIGHEST + 10;
const FILTER_MENU: usize = 1;
const WINDOW_SIZE: Size = Size { width: 800, height: 600 };
const LOADING: &str = "Loading...";
/// How often the view on screen is fetched again. GitHub asks clients not to poll the
/// notifications endpoint more than once a minute.
const POLL_INTERVAL_MS: i32 = 60_000;
/// How long the list cursor has to rest on a notification before its text is fetched, so
/// arrowing through fifty of them does not fire fifty requests.
const CONTENT_DELAY_MS: i32 = 400;

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

/// Where a notification's text comes from.
enum ContentSource {
	Issue { repo: String, number: u64 },
	Subject { url: String },
}

impl ContentSource {
	fn of(notification: &Notification) -> Option<Self> {
		if let Some(number) = notification.issue_number() {
			return Some(Self::Issue { repo: notification.repository.full_name.clone(), number });
		}
		Some(Self::Subject { url: notification.subject.url.clone()? })
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
	/// Notification bodies already fetched, by notification id.
	contents: HashMap<String, String>,
	/// The notification whose text the delay timer is about to fetch.
	pending: Option<(String, ContentSource)>,
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
			contents: HashMap::new(),
			pending: None,
		}
	}
}

#[derive(Clone)]
pub struct MainWindow {
	frame: Frame,
	views: ListBox,
	items: ListBox,
	items_label: StaticText,
	content: WebView,
	open_item: MenuItem,
	poll_timer: Rc<Timer<Frame>>,
	content_timer: Rc<Timer<Frame>>,
	state: Rc<RefCell<State>>,
}

impl MainWindow {
	pub fn open() {
		let frame = Frame::builder().with_title("Gitview").with_size(WINDOW_SIZE).build();
		let open_item = build_menu_bar(&frame);
		let panel = Panel::builder(&frame).build();
		let views_label = StaticText::builder(&panel).with_label("&Views").build();
		let views = ListBox::builder(&panel).build();
		let items_label = StaticText::builder(&panel).with_label(View::Notifications.title()).build();
		let items = ListBox::builder(&panel).build();
		let content_label = StaticText::builder(&panel).with_label("Content").build();
		let content = web::build(&panel);
		for view in View::ALL {
			views.append(view.title());
		}
		views.set_selection(0, true);
		lay_out(panel, [(views_label, views), (items_label, items)], content_label, content);
		let window = Self {
			frame,
			views,
			items,
			items_label,
			content,
			open_item,
			poll_timer: Rc::new(Timer::new(&frame)),
			content_timer: Rc::new(Timer::new(&frame)),
			state: Rc::default(),
		};
		window.bind_events();
		window.update_filter_menu();
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
			ID_OPEN => window.open_selected(),
			ID_REFRESH => window.fetch_all(),
			ID_SIGN_OUT => window.sign_out(),
			ID_EXIT => window.frame.close(false),
			id if (ID_FILTER_FIRST..ID_FILTER_FIRST + 3).contains(&id) => window.set_filter(id - ID_FILTER_FIRST),
			_ => event.skip(true),
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
		self.items.on_item_double_clicked(move |_| window.open_selected());
		let window = self.clone();
		self.items.on_selection_changed(move |_| window.show_content());
		let window = self.clone();
		self.content_timer.on_tick(move |_| window.fetch_content());
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
		if let Err(error) = token::save(&token) {
			show_warning(
				&self.frame,
				format!("Gitview could not save your sign-in, so you will need to sign in again next time. {error}"),
				"Sign-In Not Saved",
			);
		}
		self.connect(token);
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
			self.open_item.enable(false);
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
		self.open_item.enable(false);
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
		self.open_item.enable(!labels.is_empty());
		// Setting the selection in code raises no event, so the pane is refreshed by hand.
		if labels.is_empty() {
			web::show(self.content, view.title(), "", &web::paragraph(view.empty_message()));
		} else {
			self.show_content();
		}
	}

	fn open_selected(&self) {
		let Some(client) = self.state.borrow().client.clone() else {
			return;
		};
		let Some(index) = self.items.get_selection().map(|index| index as usize) else {
			return;
		};
		let state = self.state.borrow();
		let Some(loaded) = state.loaded[state.view.index()].as_ref() else {
			return;
		};
		match &loaded.items {
			Items::Notifications(notifications) => {
				let Some(notification) = notifications.get(index) else {
					return;
				};
				self.open_notification(client, notification);
			}
			Items::Repositories(repositories) => {
				let Some(repository) = repositories.get(index) else {
					return;
				};
				dialogs::open_issues(self.frame, client, repository.full_name.clone(), state.issue_filter);
			}
			Items::Issues(issues) => {
				let Some((issue, repository)) = issues.get(index).and_then(|issue| Some((issue, issue.repository()?)))
				else {
					return;
				};
				dialogs::open_issue(self.frame, client, repository.to_string(), issue.number);
			}
		}
	}

	/// Puts the selected item's text in the content pane. Everything but a notification is
	/// already in memory; a notification's own text is fetched once the cursor settles.
	fn show_content(&self) {
		let Some(index) = self.items.get_selection().map(|index| index as usize) else {
			return;
		};
		self.content_timer.stop();
		let mut state = self.state.borrow_mut();
		state.pending = None;
		let Some(loaded) = state.loaded[state.view.index()].as_ref() else {
			web::show(self.content, "Loading", "", "");
			return;
		};
		match &loaded.items {
			Items::Notifications(notifications) => {
				let Some(notification) = notifications.get(index) else {
					return;
				};
				let heading = notification.subject.title.clone();
				let meta = format!(
					"{} in {}. {}.",
					kind_label(&notification.subject.kind),
					notification.repository.full_name,
					notification.reason.replace('_', " "),
				);
				let Some(source) = ContentSource::of(notification) else {
					drop(state);
					web::show(self.content, &heading, &meta, "<p>GitHub keeps no text for this kind.</p>");
					return;
				};
				if let Some(body) = state.contents.get(&notification.id) {
					let body = body.clone();
					drop(state);
					web::show(self.content, &heading, &meta, &body);
					return;
				}
				state.pending = Some((notification.id.clone(), source));
				drop(state);
				web::show(self.content, &heading, &meta, "<p>Loading...</p>");
				self.content_timer.start(CONTENT_DELAY_MS, true);
			}
			Items::Repositories(repositories) => {
				let Some(repository) = repositories.get(index) else {
					return;
				};
				let meta = format!(
					"{}{}. {} stars, {} open issues.",
					if repository.private { "Private" } else { "Public" },
					if repository.fork { ", fork" } else { "" },
					repository.stargazers_count,
					repository.open_issues_count,
				);
				let body = format!(
					"{}<p><a href=\"https://github.com/{}\">Open on GitHub</a></p>",
					web::paragraph(repository.description.as_deref().unwrap_or("No description.")),
					repository.full_name,
				);
				let heading = repository.full_name.clone();
				drop(state);
				web::show(self.content, &heading, &meta, &body);
			}
			Items::Issues(issues) => {
				let Some(issue) = issues.get(index) else {
					return;
				};
				let heading = format!("{} #{}", issue.title, issue.number);
				let meta = format!(
					"{} in {}, {}. Opened by {}. {}.",
					if issue.is_pull_request() { "Pull request" } else { "Issue" },
					issue.repository().unwrap_or("unknown"),
					issue.state,
					issue.user.login,
					comment_count(issue.comments),
				);
				let body = body_html(issue.body_html.as_deref(), issue.body.as_deref());
				drop(state);
				web::show(self.content, &heading, &meta, &body);
			}
		}
	}

	/// Runs once the list cursor has rested on a notification, and fills the pane with what
	/// the notification points at.
	fn fetch_content(&self) {
		let (Some(client), Some((id, source))) = ({
			let mut state = self.state.borrow_mut();
			(state.client.clone(), state.pending.take())
		}) else {
			return;
		};
		let window = self.clone();
		worker::spawn(
			move || match &source {
				ContentSource::Issue { repo, number } => client
					.issue(repo, *number)
					.map(|issue| body_html(issue.body_html.as_deref(), issue.body.as_deref())),
				ContentSource::Subject { url } => {
					client.subject(url).map(|details| body_html(details.body_html.as_deref(), details.body.as_deref()))
				}
			},
			move |result| {
				let Ok(body) = result else {
					return;
				};
				window.state.borrow_mut().contents.insert(id, body);
				window.show_content();
			},
		);
	}

	/// Opens the issue thread behind a notification. Releases and the like have no thread, and
	/// their text is already in the content pane.
	fn open_notification(&self, client: Arc<Client>, notification: &Notification) {
		let read_client = Arc::clone(&client);
		let id = notification.id.clone();
		worker::spawn(move || read_client.mark_notification_read(&id), |_| {});
		if let Some(number) = notification.issue_number() {
			dialogs::open_issue(self.frame, client, notification.repository.full_name.clone(), number);
		}
	}
}

/// Builds the menu bar and returns the Open item, which is disabled while the list is empty.
fn build_menu_bar(frame: &Frame) -> MenuItem {
	let file_menu = Menu::builder().build();
	let open_item = file_menu.append(ID_OPEN, "&Open\tEnter", "Open the selected item", ItemKind::Normal);
	file_menu.append(ID_REFRESH, "&Refresh\tF5", "Reload the selected view", ItemKind::Normal);
	file_menu.append_separator();
	file_menu.append(ID_SIGN_OUT, "Sign O&ut", "Forget your GitHub sign-in", ItemKind::Normal);
	file_menu.append(ID_EXIT, "E&xit", "Close Gitview", ItemKind::Normal);
	let filter_menu = Menu::builder().build();
	frame.set_menu_bar(MenuBar::builder().append(file_menu, "&File").append(filter_menu, "F&ilter").build());
	let open_item = open_item.expect("the Open menu item");
	open_item.enable(false);
	open_item
}

// Views, then the items in the selected view, then what the selected item says: the same
// left-to-right split fedra uses for timelines and posts. Each label sits above its control so
// screen readers use it as that control's name.
fn lay_out(panel: Panel, lists: [(StaticText, ListBox); 2], content_label: StaticText, content: WebView) {
	let padding = panel.from_dip_int(wx_utils::DIALOG_PADDING);
	let sizer = BoxSizer::builder(Orientation::Horizontal).build();
	for (weight, (label, list)) in [1, 2].into_iter().zip(lists) {
		let column = BoxSizer::builder(Orientation::Vertical).build();
		column.add(&label, 0, SizerFlag::empty(), 0);
		column.add(&list, 1, SizerFlag::Expand, 0);
		sizer.add_sizer(&column, weight, SizerFlag::Expand | SizerFlag::All, padding);
	}
	let content_column = BoxSizer::builder(Orientation::Vertical).build();
	content_column.add(&content_label, 0, SizerFlag::empty(), 0);
	content_column.add(&content, 1, SizerFlag::Expand, 0);
	sizer.add_sizer(&content_column, 2, SizerFlag::Expand | SizerFlag::All, padding);
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
