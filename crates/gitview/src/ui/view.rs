//! The lists the main window can show, and how they are named in the config file.

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

	/// The filters this view offers, in menu order. Repository lists have none.
	pub const fn filters(self) -> &'static [&'static str] {
		match self {
			Self::Notifications => &["&Unread", "&All"],
			Self::Repositories | Self::Starred | Self::Watched => &[],
			Self::Assigned | Self::ReviewRequested => &["&Open", "&Closed", "&All"],
		}
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
	use super::{View, arrange};

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
