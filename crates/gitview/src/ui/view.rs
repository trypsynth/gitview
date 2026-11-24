//! The lists the main window can show, and how they are named in the config file.

use std::collections::BTreeMap;

use gitview_core::models::{IssueKind, IssueState, RepositoryKind, RepositoryOrder, SearchOrder, StarredOrder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
	Notifications,
	Repositories,
	Starred,
	Watched,
	Assigned,
	ReviewRequested,
}

impl View {
	pub const ALL: [Self; 6] =
		[Self::Notifications, Self::Repositories, Self::Starred, Self::Watched, Self::Assigned, Self::ReviewRequested];
	pub const COUNT: usize = Self::ALL.len();

	pub const fn index(self) -> usize {
		match self {
			Self::Notifications => 0,
			Self::Repositories => 1,
			Self::Starred => 2,
			Self::Watched => 3,
			Self::Assigned => 4,
			Self::ReviewRequested => 5,
		}
	}

	pub const fn title(self) -> &'static str {
		match self {
			Self::Notifications => "Notifications",
			Self::Repositories => "Repositories",
			Self::Starred => "Starred",
			Self::Watched => "Watching",
			Self::Assigned => "Assigned to me",
			Self::ReviewRequested => "Review requested",
		}
	}

	/// The filters this view offers, in menu order, the first being its default.
	pub const fn filters(self) -> &'static [Filter] {
		match self {
			Self::Notifications => &[Filter::Unread, Filter::All],
			Self::Repositories => &[Filter::All, Filter::Owned, Filter::Public, Filter::Private],
			Self::Starred | Self::Watched => &[],
			Self::Assigned | Self::ReviewRequested => &[Filter::Open, Filter::Closed, Filter::All],
		}
	}

	/// The filter this view has until the user picks another.
	pub fn default_filter(self) -> Filter {
		self.filters().first().copied().unwrap_or(Filter::All)
	}

	/// The filter the settings file says this view shows, or its default when it names none the
	/// view offers.
	pub fn saved_filter(self, filters: &BTreeMap<String, String>) -> Filter {
		filters
			.get(self.id())
			.and_then(|id| self.filters().iter().copied().find(|filter| filter.id() == id))
			.unwrap_or_else(|| self.default_filter())
	}

	/// What the list says when it's empty, which in Assigned to me depends on whether it lists
	/// issues, pull requests, or both.
	pub const fn empty_message_for(self, kind: IssueKind) -> &'static str {
		match (self, kind) {
			(Self::Assigned, IssueKind::Issues) => "No issues assigned to you.",
			(Self::Assigned, IssueKind::PullRequests) => "No pull requests assigned to you.",
			_ => self.empty_message(),
		}
	}

	/// The orders this view can be sorted in, in menu order, the first being its default. Some
	/// views have only the one GitHub gives them in.
	pub const fn sorts(self) -> &'static [Sort] {
		match self {
			Self::Repositories => &[Sort::Updated, Sort::Pushed, Sort::Created, Sort::Name],
			Self::Starred => &[Sort::Starred, Sort::Updated],
			Self::Assigned | Self::ReviewRequested => &[Sort::Updated, Sort::Opened, Sort::Comments],
			Self::Notifications | Self::Watched => &[],
		}
	}

	/// The order this view is sorted in until the user picks another, which is how GitHub gives it.
	pub fn default_sort(self) -> Sort {
		self.sorts().first().copied().unwrap_or(Sort::Updated)
	}

	/// The order the settings file says this view is sorted in, or its default when it names none
	/// the view offers.
	pub fn saved_sort(self, sorts: &BTreeMap<String, String>) -> Sort {
		sorts
			.get(self.id())
			.and_then(|id| self.sorts().iter().copied().find(|sort| sort.id() == id))
			.unwrap_or_else(|| self.default_sort())
	}

	pub const fn empty_message(self) -> &'static str {
		match self {
			Self::Notifications => "No notifications.",
			Self::Repositories => "No repositories.",
			Self::Starred => "No starred repositories.",
			Self::Watched => "You aren't watching any repositories.",
			Self::Assigned => "Nothing assigned to you.",
			Self::ReviewRequested => "No reviews requested from you.",
		}
	}

	/// The name the config file knows this view by, which stays put when titles change.
	pub const fn id(self) -> &'static str {
		match self {
			Self::Notifications => "notifications",
			Self::Repositories => "repositories",
			Self::Starred => "starred",
			Self::Watched => "watched",
			Self::Assigned => "assigned",
			Self::ReviewRequested => "review_requested",
		}
	}

	pub fn from_id(id: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|view| view.id() == id)
	}
}

/// Every view in the order the user chose, and whether each is shown. `order` and `hidden` are
/// view ids from the config file. Views missing from `order` go at the end, shown, so a view
/// added in a later version turns up without being looked for.
pub fn arrange(order: &[String], hidden: &[String]) -> Vec<(View, bool)> {
	let mut views: Vec<View> = Vec::with_capacity(View::COUNT);
	for view in order.iter().filter_map(|id| View::from_id(id)).chain(View::ALL) {
		if !views.contains(&view) {
			views.push(view);
		}
	}
	views.into_iter().map(|view| (view, !hidden.iter().any(|id| id == view.id()))).collect()
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;

	use super::{Filter, Sort, View, arrange, issue_kind_id, saved_issue_kind};

	#[test]
	fn the_assigned_kind_is_saved_and_read_back_and_defaults_to_both() {
		use gitview_core::models::IssueKind;
		for kind in [IssueKind::Both, IssueKind::Issues, IssueKind::PullRequests] {
			assert_eq!(saved_issue_kind(Some(issue_kind_id(kind))), kind);
		}
		assert_eq!(issue_kind_id(IssueKind::PullRequests), "pull_requests");
		assert_eq!(saved_issue_kind(Some("discussions")), IssueKind::Both);
		assert_eq!(saved_issue_kind(None), IssueKind::Both);
	}

	#[test]
	fn a_saved_sort_is_used_only_when_the_view_offers_it() {
		let saved = BTreeMap::from([
			("starred".to_owned(), "updated".to_owned()),
			("repositories".to_owned(), "comments".to_owned()),
		]);
		assert_eq!(View::Starred.saved_sort(&saved), Sort::Updated);
		assert_eq!(View::Repositories.saved_sort(&saved), Sort::Updated);
		assert_eq!(View::Assigned.saved_sort(&saved), Sort::Updated);
		assert_eq!(View::Starred.saved_sort(&BTreeMap::new()), Sort::Starred);
	}

	#[test]
	fn a_saved_filter_is_used_only_when_the_view_offers_it() {
		let saved = BTreeMap::from([
			("repositories".to_owned(), "private".to_owned()),
			("notifications".to_owned(), "closed".to_owned()),
		]);
		assert_eq!(View::Repositories.saved_filter(&saved), Filter::Private);
		assert_eq!(View::Notifications.saved_filter(&saved), Filter::Unread);
		assert_eq!(View::Assigned.saved_filter(&saved), Filter::Open);
		assert_eq!(View::Starred.saved_filter(&saved), Filter::All);
	}

	fn ids(ids: &[&str]) -> Vec<String> {
		ids.iter().map(|id| (*id).to_owned()).collect()
	}

	#[test]
	fn no_settings_shows_every_view_in_the_usual_order() {
		assert_eq!(arrange(&[], &[]), View::ALL.map(|view| (view, true)).to_vec());
	}

	#[test]
	fn the_chosen_order_comes_first_and_unknown_ids_are_ignored() {
		let arranged = arrange(&ids(&["starred", "gone", "notifications", "starred"]), &ids(&["notifications"]));
		assert_eq!(arranged[..3], [(View::Starred, true), (View::Notifications, false), (View::Repositories, true)]);
		assert_eq!(arranged.len(), View::COUNT);
	}
}

/// An order a view can be sorted in. Which ones a view offers is up to [`View::sorts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
	Updated,
	Pushed,
	Created,
	Name,
	Starred,
	Opened,
	Comments,
}

impl Sort {
	/// What the settings file calls it.
	pub const fn id(self) -> &'static str {
		match self {
			Self::Updated => "updated",
			Self::Pushed => "pushed",
			Self::Created => "created",
			Self::Name => "name",
			Self::Starred => "starred",
			Self::Opened => "opened",
			Self::Comments => "comments",
		}
	}

	/// What the Sort menu calls it.
	pub const fn label(self) -> &'static str {
		match self {
			Self::Updated => "Last &updated",
			Self::Pushed => "Last &pushed",
			Self::Created => "Date &created",
			Self::Name => "&Name",
			Self::Starred => "Date &starred",
			Self::Opened => "Date &opened",
			Self::Comments => "Most &commented",
		}
	}

	pub const fn repository_order(self) -> RepositoryOrder {
		match self {
			Self::Pushed => RepositoryOrder::Pushed,
			Self::Created => RepositoryOrder::Created,
			Self::Name => RepositoryOrder::Name,
			_ => RepositoryOrder::Updated,
		}
	}

	pub const fn starred_order(self) -> StarredOrder {
		match self {
			Self::Updated => StarredOrder::Updated,
			_ => StarredOrder::Starred,
		}
	}

	pub const fn search_order(self) -> SearchOrder {
		match self {
			Self::Opened => SearchOrder::Created,
			Self::Comments => SearchOrder::Comments,
			_ => SearchOrder::Updated,
		}
	}
}

/// What a view can be narrowed to. Which ones a view offers is up to [`View::filters`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
	All,
	Unread,
	Open,
	Closed,
	Owned,
	Public,
	Private,
}

impl Filter {
	/// What the settings file calls it.
	pub const fn id(self) -> &'static str {
		match self {
			Self::All => "all",
			Self::Unread => "unread",
			Self::Open => "open",
			Self::Closed => "closed",
			Self::Owned => "owned",
			Self::Public => "public",
			Self::Private => "private",
		}
	}

	/// What the Filter menu calls it.
	pub const fn label(self) -> &'static str {
		match self {
			Self::All => "&All",
			Self::Unread => "&Unread",
			Self::Open => "&Open",
			Self::Closed => "&Closed",
			Self::Owned => "&Owned by me",
			Self::Public => "&Public",
			Self::Private => "Pri&vate",
		}
	}

	pub const fn issue_state(self) -> IssueState {
		match self {
			Self::Closed => IssueState::Closed,
			Self::All => IssueState::All,
			_ => IssueState::Open,
		}
	}

	pub const fn repository_kind(self) -> RepositoryKind {
		match self {
			Self::Owned => RepositoryKind::Owned,
			Self::Public => RepositoryKind::Public,
			Self::Private => RepositoryKind::Private,
			_ => RepositoryKind::All,
		}
	}
}

/// The kinds Assigned to me can list, in the Filter menu's order, with what the menu and the
/// settings file call them.
pub const ISSUE_KINDS: [(IssueKind, &str, &str); 3] = [
	(IssueKind::Both, "&Both", "both"),
	(IssueKind::Issues, "&Issues", "issues"),
	(IssueKind::PullRequests, "&Pull requests", "pull_requests"),
];

/// The kind the settings file names, or both when it names none Gitview knows.
pub fn saved_issue_kind(id: Option<&str>) -> IssueKind {
	ISSUE_KINDS.iter().find(|(_, _, name)| Some(*name) == id).map_or(IssueKind::Both, |(kind, _, _)| *kind)
}

/// What the settings file calls `kind`.
pub fn issue_kind_id(kind: IssueKind) -> &'static str {
	ISSUE_KINDS.iter().find(|(each, _, _)| *each == kind).map_or("both", |(_, _, name)| name)
}
