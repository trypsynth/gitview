//! A user's own dialog: their profile, then their repositories, stars and the people around them,
//! one list per section, as a repository's dialog lists its own.

use std::{cell::Cell, rc::Rc, sync::Arc};

use gitview_core::{
	Client,
	models::{Repository, User, UserProfile},
};
use quiet_list::Select;
use wx_utils::{
	dialog_padding, show_error,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::{
	prelude::*,
	utils::{BrowserLaunchFlags, launch_default_browser},
};

use super::open_repository;
use crate::{
	config::{self, Lists},
	ui::{
		feed::{self, Feed, FeedSection, Rows, Shared},
		notice,
		sectioned::SectionedLists,
		text::{local_time, repository_label},
	},
};

const LIST_SIZE: Size = Size { width: 600, height: 400 };

/// What a user's dialog lists, in the order its Sections list shows them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
	Profile,
	Repositories,
	Starred,
	Followers,
	Following,
}

impl FeedSection for Section {
	fn title(self) -> &'static str {
		match self {
			Self::Profile => "Profile",
			Self::Repositories => "Repositories",
			Self::Starred => "Starred",
			Self::Followers => "Followers",
			Self::Following => "Following",
		}
	}

	fn empty_message(self) -> &'static str {
		match self {
			Self::Profile => "No profile.",
			Self::Repositories => "No public repositories.",
			Self::Starred => "Nothing starred.",
			Self::Followers => "No followers.",
			Self::Following => "Not following anyone.",
		}
	}

	/// The profile reads top to bottom, and people always come most recent first: GitHub gives no
	/// way to start from the other end of a famous account's thousands of followers.
	fn follows_order(self) -> bool {
		matches!(self, Self::Repositories | Self::Starred)
	}
}

impl Section {
	const ALL: [Self; 5] = [Self::Profile, Self::Repositories, Self::Starred, Self::Followers, Self::Following];
}

/// Loads `login`'s profile and shows their dialog, or says there's no such user.
pub fn open_user<P: WxWidget + WxEvtHandler + Copy + 'static>(parent: P, client: Arc<Client>, login: String) {
	let (lookup_client, lookup_login) = (Arc::clone(&client), login.clone());
	let lookup = background_blocking(move || lookup_client.user(&lookup_login));
	spawn_for(&parent, async move {
		match lookup.await {
			Ok(profile) => show_user_dialog(&parent, client, &profile),
			Err(error) if error.is_not_found() => {
				show_error(&parent, format!("GitHub has no user {login}."), "Go to User");
			}
			Err(error) => notice::error(&parent, error, "Could Not Open User"),
		}
	});
}

fn show_user_dialog(parent: &dyn WxWidget, client: Arc<Client>, profile: &UserProfile) {
	let login = profile.login.clone();
	let title = profile
		.name
		.as_deref()
		.filter(|name| !name.is_empty())
		.map_or_else(|| login.clone(), |name| format!("{name} ({login})"));
	let dialog = Dialog::builder(parent, &title).build();
	let padding = dialog_padding(&dialog);
	// An organization stars nothing and follows no one.
	let shown: Vec<_> = Section::ALL
		.into_iter()
		.filter(|section| !profile.is_organization() || !matches!(section, Section::Starred | Section::Following))
		.map(|section| (section, section.title()))
		.collect();
	let sections = SectionedLists::new(&dialog, "Sections", &shown);
	let lists = config::load().lists;
	for (section, list) in sections.lists() {
		list.set_min_size(LIST_SIZE);
		list.set_start_at_end(lists.oldest_first && section.follows_order());
	}
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	feed::fill(sections.list(Section::Profile), profile_rows(profile), "", Select::Start);
	let feeds = Rc::new(Feeds::new(&sections, lists, profile.is_organization()));
	feed::on_reaching_oldest(&sections, lists, &feeds.at_end(dialog, &client));
	// A repository opens as its own dialog, a person as theirs, and a link in the profile in the
	// browser.
	let open: Rc<dyn Fn(Section, String)> = {
		let client = Arc::clone(&client);
		Rc::new(move |section, key| match section {
			Section::Profile => {
				if key.starts_with("https://") || key.starts_with("http://") {
					launch_default_browser(&key, BrowserLaunchFlags::Default);
				}
			}
			Section::Repositories | Section::Starred => open_repository(dialog, Arc::clone(&client), key),
			Section::Followers | Section::Following => open_user(dialog, Arc::clone(&client), key),
		})
	};
	feed::on_open(&sections, &open);
	feeds.start(Section::Repositories, dialog, &client, &login);
	// The rest wait until someone looks: a popular account's followers run to thousands.
	// The profile is filled and the repositories asked for already.
	let started = Cell::new(Section::ALL.map(|section| matches!(section, Section::Profile | Section::Repositories)));
	sections.on_section_changed(move |section| {
		let index = Section::ALL.iter().position(|each| *each == section).unwrap_or_default();
		let mut seen = started.get();
		if !std::mem::replace(&mut seen[index], true) {
			started.set(seen);
			feeds.start(section, dialog, &client, &login);
		}
	});
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add_sizer(&sections.sizer(3), 1, SizerFlag::Expand, 0);
	content.add(
		&close_button,
		0,
		SizerFlag::AlignRight | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
		padding,
	);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	sections.current_list().set_focus();
	dialog.show_modal();
	dialog.destroy();
}

/// The profile as rows, each keyed by what it is, and links by where they go so Enter can open
/// them.
fn profile_rows(profile: &UserProfile) -> Rows {
	let mut rows = Vec::new();
	let mut add = |key: &str, label: String| rows.push((key.to_owned(), label));
	let present =
		|text: &Option<String>| text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned);
	if let Some(name) = present(&profile.name) {
		add("name", format!("Name: {name}"));
	}
	if profile.is_organization() {
		add("kind", "Organization".to_owned());
	}
	if let Some(bio) = present(&profile.bio) {
		add("bio", format!("Bio: {bio}"));
	}
	if let Some(company) = present(&profile.company) {
		add("company", format!("Company: {company}"));
	}
	if let Some(location) = present(&profile.location) {
		add("location", format!("Location: {location}"));
	}
	if let Some(blog) = present(&profile.blog) {
		let url = if blog.contains("://") { blog } else { format!("https://{blog}") };
		add(&url, format!("Website: {url}"));
	}
	add("followers", format!("Followers: {}", profile.followers));
	add("following", format!("Following: {}", profile.following));
	add("repositories", format!("Public repositories: {}", profile.public_repos));
	add("joined", format!("Joined {}", local_time(&profile.created_at)));
	add(&profile.html_url, "Profile on GitHub".to_owned());
	rows
}

/// The dialog's lists that GitHub hands out a page at a time.
struct Feeds {
	repositories: Shared<Section, Repository>,
	starred: Shared<Section, Repository>,
	followers: Shared<Section, User>,
	following: Shared<Section, User>,
	/// Whether the dialog is an organization's, whose followers GitHub lists differently.
	organization: bool,
}

impl Feeds {
	fn new(sections: &SectionedLists<Section>, lists: Lists, organization: bool) -> Self {
		let only = |section| move |shown| shown == section;
		let repository_row =
			|_, repository: &Repository| Some((repository.full_name.clone(), repository_label(repository)));
		let user_row = |_, user: &User| {
			let label =
				if user.is_organization() { format!("{}, organization", user.login) } else { user.login.clone() };
			Some((user.login.clone(), label))
		};
		Self {
			repositories: Feed::new(sections, lists, only(Section::Repositories), repository_row),
			starred: Feed::new(sections, lists, only(Section::Starred), repository_row),
			followers: Feed::new(sections, lists, only(Section::Followers), user_row),
			following: Feed::new(sections, lists, only(Section::Following), user_row),
			organization,
		}
	}

	/// Loads more of a section's feed when its oldest row is reached. It holds the feeds weakly,
	/// since the lists hold it and the feeds hold the lists.
	fn at_end(&self, dialog: Dialog, client: &Arc<Client>) -> Rc<dyn Fn(Section)> {
		let (repositories, starred) = (Rc::downgrade(&self.repositories), Rc::downgrade(&self.starred));
		let (followers, following) = (Rc::downgrade(&self.followers), Rc::downgrade(&self.following));
		let client = Arc::clone(client);
		Rc::new(move |section| {
			let repository_feed = match section {
				Section::Repositories => &repositories,
				Section::Starred => &starred,
				_ => {
					let user_feed = match section {
						Section::Followers => &followers,
						Section::Following => &following,
						_ => return,
					};
					if let Some(feed) = user_feed.upgrade() {
						feed::load_more(dialog, &client, &feed, section);
					}
					return;
				}
			};
			if let Some(feed) = repository_feed.upgrade() {
				feed::load_more(dialog, &client, &feed, section);
			}
		})
	}

	/// Loads the first page of `section`'s list of what `login` has.
	fn start(&self, section: Section, dialog: Dialog, client: &Arc<Client>, login: &str) {
		let (fetch, login, organization) = (Arc::clone(client), login.to_owned(), self.organization);
		let client = Arc::clone(client);
		match section {
			Section::Profile => {}
			Section::Repositories => feed::start(
				dialog,
				client,
				Rc::clone(&self.repositories),
				move || fetch.user_repositories(&login),
				"Could Not Load Repositories",
			),
			Section::Starred => feed::start(
				dialog,
				client,
				Rc::clone(&self.starred),
				move || fetch.user_starred(&login),
				"Could Not Load Starred Repositories",
			),
			Section::Followers => feed::start(
				dialog,
				client,
				Rc::clone(&self.followers),
				move || if organization { fetch.organization_followers(&login) } else { fetch.followers(&login) },
				"Could Not Load Followers",
			),
			Section::Following => feed::start(
				dialog,
				client,
				Rc::clone(&self.following),
				move || fetch.following(&login),
				"Could Not Load Following",
			),
		}
	}
}
