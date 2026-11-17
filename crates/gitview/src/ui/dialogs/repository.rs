//! A repository's own dialog: what it holds, one list per section, from its commits to its
//! closed pull requests.

use std::{
	cell::{Cell, RefCell},
	fmt::Write,
	rc::{Rc, Weak},
	sync::Arc,
};

use gitview_core::{
	Client, Cursor, Error, Page,
	models::{ChangedFile, Commit, CommitDetails, Issue, IssueState},
};
use quiet_list::{QuietList, Select};
use serde::de::DeserializeOwned;
use wx_utils::{
	dialog_padding,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use super::{is_plain_enter, open_issue, page::show_page};
use crate::ui::{
	notice,
	sectioned::SectionedLists,
	text::{comment_count, count, local_time},
	web,
};

const LIST_SIZE: Size = Size { width: 600, height: 400 };
/// The key of the row shown in place of items. It is always the only row, so it cannot be
/// mistaken for an item's key, which is a commit's hash or an issue's number.
const MESSAGE_KEY: &str = "";

/// What a repository's dialog lists, in the order its Sections list shows them. Open and closed
/// issues are sections of their own rather than a filter, so each keeps its own place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
	Commits,
	Issues,
	PullRequests,
	ClosedIssues,
	ClosedPullRequests,
}

impl Section {
	const ALL: [Self; 5] =
		[Self::Commits, Self::Issues, Self::PullRequests, Self::ClosedIssues, Self::ClosedPullRequests];

	const fn title(self) -> &'static str {
		match self {
			Self::Commits => "Commits",
			Self::Issues => "Issues",
			Self::PullRequests => "Pull requests",
			Self::ClosedIssues => "Closed issues",
			Self::ClosedPullRequests => "Closed pull requests",
		}
	}

	/// Which issues the section lists, or `None` for commits.
	const fn issue_state(self) -> Option<IssueState> {
		match self {
			Self::Commits => None,
			Self::Issues | Self::PullRequests => Some(IssueState::Open),
			Self::ClosedIssues | Self::ClosedPullRequests => Some(IssueState::Closed),
		}
	}

	const fn lists_pull_requests(self) -> bool {
		matches!(self, Self::PullRequests | Self::ClosedPullRequests)
	}

	const fn empty_message(self) -> &'static str {
		match self {
			Self::Commits => "No commits.",
			Self::Issues => "No open issues.",
			Self::PullRequests => "No open pull requests.",
			Self::ClosedIssues => "No closed issues.",
			Self::ClosedPullRequests => "No closed pull requests.",
		}
	}
}

/// Shows `repo` (`owner/name`), loading its commits and open issues while the dialog is up, and
/// its closed issues the first time a section of them is chosen. A list stays empty until what
/// it lists arrives.
pub fn open_repository<P: WxWidget + WxEvtHandler + Copy + 'static>(parent: P, client: Arc<Client>, repo: String) {
	let dialog = Dialog::builder(&parent, &repo).build();
	let padding = dialog_padding(&dialog);
	let sections = SectionedLists::new(&dialog, "Sections", &Section::ALL.map(|section| (section, section.title())));
	for (_, list) in sections.lists() {
		list.set_min_size(LIST_SIZE);
	}
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let commits = Feed::new(
		&sections,
		|section| section == Section::Commits,
		|_, commit: &Commit| Some((commit.sha.clone(), commit_label(commit))),
	);
	let open_issues = Feed::new(&sections, |section| section.issue_state() == Some(IssueState::Open), issue_row);
	let closed_issues = Feed::new(&sections, |section| section.issue_state() == Some(IssueState::Closed), issue_row);
	// Held weakly, since the lists hold this and the feeds hold the lists. The feeds themselves
	// live as long as this function does, which is as long as the dialog is up.
	let at_end: Rc<dyn Fn(Section)> = {
		let (commits, open_issues, closed_issues) =
			(Rc::downgrade(&commits), Rc::downgrade(&open_issues), Rc::downgrade(&closed_issues));
		let client = Arc::clone(&client);
		Rc::new(move |section| match section.issue_state() {
			None => {
				if let Some(feed) = commits.upgrade() {
					load_more(dialog, &client, &feed, section);
				}
			}
			Some(IssueState::Open) => {
				if let Some(feed) = open_issues.upgrade() {
					load_more(dialog, &client, &feed, section);
				}
			}
			Some(_) => {
				if let Some(feed) = closed_issues.upgrade() {
					load_more(dialog, &client, &feed, section);
				}
			}
		})
	};
	bind_lists(dialog, &sections, &client, &repo, &at_end);
	let (commits_client, commits_repo) = (Arc::clone(&client), repo.clone());
	start(
		dialog,
		Arc::clone(&client),
		Rc::clone(&commits),
		move || commits_client.commits(&commits_repo),
		"Could Not Load Commits",
	);
	let (open_client, open_repo) = (Arc::clone(&client), repo.clone());
	start(
		dialog,
		Arc::clone(&client),
		Rc::clone(&open_issues),
		move || open_client.issues(&open_repo, IssueState::Open),
		"Could Not Load Issues",
	);
	// Closed issues can run to hundreds, so they wait until someone looks.
	let closed_requested = Cell::new(false);
	sections.on_section_changed(move |section| {
		if section.issue_state() == Some(IssueState::Closed) && !closed_requested.replace(true) {
			let (closed_client, closed_repo) = (Arc::clone(&client), repo.clone());
			start(
				dialog,
				Arc::clone(&client),
				Rc::clone(&closed_issues),
				move || closed_client.issues(&closed_repo, IssueState::Closed),
				"Could Not Load Issues",
			);
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

/// Opens the selected item on Enter or a double-click: a commit as a page, an issue or pull
/// request as its thread, and loads more of a section when its last row is reached. The lists'
/// own callbacks hold them weakly, so they can go once the dialog closes.
fn bind_lists(
	dialog: Dialog,
	sections: &SectionedLists<Section>,
	client: &Arc<Client>,
	repo: &str,
	at_end: &Rc<dyn Fn(Section)>,
) {
	for (section, list) in sections.lists() {
		let (reached, at_end) = (Rc::downgrade(list), Rc::clone(at_end));
		list.on_selection_changed(move || {
			if let Some(list) = reached.upgrade()
				&& list.selected_index().is_some_and(|index| index + 1 == list.len())
			{
				at_end(section);
			}
		});
		let open = {
			let (list, client, repo) = (Rc::downgrade(list), Arc::clone(client), repo.to_owned());
			Rc::new(move || {
				// The message row's key is empty, so it opens nothing.
				let Some(key) = Weak::upgrade(&list).and_then(|list| list.selected()).filter(|key| !key.is_empty())
				else {
					return;
				};
				if section == Section::Commits {
					open_commit(dialog, Arc::clone(&client), repo.clone(), key);
				} else if let Ok(number) = key.parse() {
					open_issue(dialog, Arc::clone(&client), repo.clone(), number);
				}
			})
		};
		let open_on_activate = Rc::clone(&open);
		list.on_activate(move || open_on_activate());
		list.on_key_down(move |event| {
			let enter = is_plain_enter(event);
			if enter {
				open();
			}
			enter
		});
	}
}

/// Rows as a list shows them: each item's key and label.
type Rows = Vec<(String, String)>;

/// How many pages one reach for more goes through looking for something for the section that
/// asked, before settling for what it found. Issues and pull requests share pages, so a section
/// can come up empty on a page full of the other kind.
const PAGES_PER_LOAD: usize = 10;

/// A list GitHub hands out a page at a time, and the sections it feeds: commits feed one, issues
/// feed both the issue and the pull request sections.
struct Feed<T> {
	lists: Vec<(Section, Rc<QuietList<String>>, Rows)>,
	next: Option<Cursor<T>>,
	/// Set while pages are on their way, so reaching the end again doesn't ask twice.
	loading: bool,
	/// The row an item makes in a section, or `None` when the section doesn't list it.
	row: fn(Section, &T) -> Option<(String, String)>,
}

/// A feed the dialog and its loading tasks share.
type Shared<T> = Rc<RefCell<Feed<T>>>;

impl<T> Feed<T> {
	fn new(
		sections: &SectionedLists<Section>,
		feeds: impl Fn(Section) -> bool,
		row: fn(Section, &T) -> Option<(String, String)>,
	) -> Shared<T> {
		let lists = sections
			.lists()
			.filter(|(section, _)| feeds(*section))
			.map(|(section, list)| (section, Rc::clone(list), Vec::new()))
			.collect();
		Rc::new(RefCell::new(Self { lists, next: None, loading: false, row }))
	}

	/// Adds `items` to the sections that list them, and says which sections gained rows.
	fn add(&mut self, items: &[T]) -> Vec<Section> {
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

	fn empty_sections(&self) -> Vec<Section> {
		self.lists.iter().filter(|(_, _, rows)| rows.is_empty()).map(|(section, _, _)| *section).collect()
	}
}

/// Puts each section's rows in its list, or its message when it has none.
///
/// The feed is let go before any list is filled: filling a list runs its selection callback,
/// and reaching a list's end from there loads more of this same feed.
fn show<T>(feed: &Shared<T>, select: &Select<String>) {
	let shown: Vec<_> = feed
		.borrow()
		.lists
		.iter()
		.map(|(section, list, rows)| (Rc::clone(list), rows.clone(), section.empty_message()))
		.collect();
	for (list, rows, message) in shown {
		fill(&list, rows, message, select.clone());
	}
}

/// Says in each of the feed's lists that its first page couldn't be loaded.
fn show_failure<T>(feed: &Shared<T>) {
	let lists: Vec<_> = feed.borrow().lists.iter().map(|(section, list, _)| (*section, Rc::clone(list))).collect();
	for (section, list) in lists {
		let message = format!("Could not load {}.", section.title().to_lowercase());
		fill(&list, Vec::new(), &message, Select::Start);
	}
}

/// Fills `feed` from its first page, then goes on for any section that page left empty.
fn start<T: DeserializeOwned + Send + 'static>(
	dialog: Dialog,
	client: Arc<Client>,
	feed: Shared<T>,
	first: impl FnOnce() -> Result<Page<T>, Error> + Send + 'static,
	failed_title: &'static str,
) {
	feed.borrow_mut().loading = true;
	let fetch = background_blocking(first);
	spawn_for(&dialog, async move {
		match fetch.await {
			Ok(page) => {
				let empty = {
					let mut inner = feed.borrow_mut();
					inner.add(&page.items);
					inner.next = page.next;
					inner.empty_sections()
				};
				show(&feed, &Select::Start);
				let result = read_on(&client, &feed, empty).await;
				feed.borrow_mut().loading = false;
				show(&feed, &Select::Keep);
				if let Err(error) = result {
					notice::error(&dialog, error, failed_title);
				}
			}
			Err(error) => {
				feed.borrow_mut().loading = false;
				show_failure(&feed);
				notice::error(&dialog, error, failed_title);
			}
		}
	});
}

/// Loads more of `feed` for `section`, whose last row was reached, as Fedra does at the end of a
/// timeline. The selection stays where it is, and the new rows are there when it moves on.
fn load_more<T: DeserializeOwned + Send + 'static>(
	dialog: Dialog,
	client: &Arc<Client>,
	feed: &Shared<T>,
	section: Section,
) {
	{
		let mut inner = feed.borrow_mut();
		if inner.loading || inner.next.is_none() {
			return;
		}
		inner.loading = true;
	}
	let (client, feed) = (Arc::clone(client), Rc::clone(feed));
	spawn_for(&dialog, async move {
		let result = read_on(&client, &feed, vec![section]).await;
		feed.borrow_mut().loading = false;
		show(&feed, &Select::Keep);
		if let Err(error) = result {
			notice::error(&dialog, error, "Could Not Load More");
		}
	});
}

/// Fetches the pages after those `feed` has while any of `wanted` has gained nothing, up to
/// [`PAGES_PER_LOAD`], and stops at the list's end.
#[expect(clippy::future_not_send, reason = "it runs on the UI thread, where the feeds live")]
async fn read_on<T: DeserializeOwned + Send + 'static>(
	client: &Arc<Client>,
	feed: &Shared<T>,
	mut wanted: Vec<Section>,
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
		let gained = inner.add(&page.items);
		inner.next = page.next;
		wanted.retain(|section| !gained.contains(section));
	}
	Ok(())
}

fn issue_row(section: Section, issue: &Issue) -> Option<(String, String)> {
	(issue.is_pull_request() == section.lists_pull_requests()).then(|| (issue.number.to_string(), issue_label(issue)))
}

/// Puts `rows` in `list`, or `message` alone when there are none.
fn fill(list: &QuietList<String>, rows: Rows, message: &str, select: Select<String>) {
	let rows = if rows.is_empty() { vec![(MESSAGE_KEY.to_owned(), message.to_owned())] } else { rows };
	list.update_labelled(rows, select);
}

/// Loads commit `sha` with what it changed, and shows it as a page.
fn open_commit(dialog: Dialog, client: Arc<Client>, repo: String, sha: String) {
	let fetch = background_blocking(move || client.commit(&repo, &sha));
	spawn_for(&dialog, async move {
		match fetch.await {
			Ok(details) => {
				let commit = &details.commit;
				let meta = format!("{} committed on {}", commit.author_name(), local_time(&commit.commit.author.date));
				show_page(&dialog, commit.summary(), &meta, &commit_page(&details));
			}
			Err(error) => notice::error(&dialog, error, "Could Not Open Commit"),
		}
	});
}

/// An issue or pull request as its section lists it. A closed one says how it ended.
fn issue_label(issue: &Issue) -> String {
	let mut label =
		format!("#{} {}, by {}, {}", issue.number, issue.title, issue.user.login, comment_count(issue.comments));
	if !issue.is_open() {
		let ending = if issue.is_pull_request() {
			if issue.is_merged() { "merged".to_owned() } else { "closed".to_owned() }
		} else {
			issue.state_reason.as_deref().unwrap_or("closed").replace('_', " ")
		};
		label.push_str(", ");
		label.push_str(&ending);
	}
	label
}

fn commit_label(commit: &Commit) -> String {
	format!(
		"{}, by {}, {}, {}",
		commit.summary(),
		commit.author_name(),
		local_time(&commit.commit.author.date),
		commit.short_sha()
	)
}

/// The page under a commit's heading: the rest of its message, then what it changed, file by
/// file.
fn commit_page(details: &CommitDetails) -> String {
	let mut page = String::new();
	let description = details.commit.description();
	if !description.is_empty() {
		page.push_str(&web::paragraph(description));
	}
	let files = u64::try_from(details.files.len()).unwrap_or(u64::MAX);
	// Writing to a String can't fail.
	let _ = write!(
		page,
		"<h2>Changes</h2><p>{}, {} and {}.</p><ul>",
		count(files, "file changed", "files changed"),
		count(details.stats.additions, "addition", "additions"),
		count(details.stats.deletions, "deletion", "deletions"),
	);
	for file in &details.files {
		let _ = write!(page, "<li>{}</li>", web::escape(&file_line(file)));
	}
	page.push_str("</ul>");
	page
}

/// How one file changed, such as "Modified src/main.rs: 3 additions, 1 deletion".
fn file_line(file: &ChangedFile) -> String {
	let what = match (file.status.as_str(), &file.previous_filename) {
		("renamed", Some(old)) => format!("Renamed {old} to {}", file.filename),
		("added", _) => format!("Added {}", file.filename),
		("removed", _) => format!("Deleted {}", file.filename),
		("modified", _) => format!("Modified {}", file.filename),
		("copied", _) => format!("Copied {}", file.filename),
		_ => format!("Changed {}", file.filename),
	};
	format!(
		"{what}: {}, {}",
		count(file.additions, "addition", "additions"),
		count(file.deletions, "deletion", "deletions")
	)
}

#[cfg(test)]
mod tests {
	use gitview_core::models::ChangedFile;

	use super::file_line;

	fn file(status: &str, previous: Option<&str>, additions: u64, deletions: u64) -> ChangedFile {
		ChangedFile {
			filename: "src/main.rs".to_owned(),
			status: status.to_owned(),
			additions,
			deletions,
			previous_filename: previous.map(str::to_owned),
			patch: None,
		}
	}

	#[test]
	fn a_file_line_says_what_happened_and_how_much() {
		assert_eq!(file_line(&file("modified", None, 3, 1)), "Modified src/main.rs: 3 additions, 1 deletion");
		assert_eq!(file_line(&file("added", None, 1, 0)), "Added src/main.rs: 1 addition, 0 deletions");
		assert_eq!(file_line(&file("removed", None, 0, 9)), "Deleted src/main.rs: 0 additions, 9 deletions");
		assert_eq!(
			file_line(&file("renamed", Some("src/old.rs"), 0, 0)),
			"Renamed src/old.rs to src/main.rs: 0 additions, 0 deletions"
		);
		assert_eq!(file_line(&file("changed", None, 2, 2)), "Changed src/main.rs: 2 additions, 2 deletions");
	}
}
