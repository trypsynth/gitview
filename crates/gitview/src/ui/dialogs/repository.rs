//! A repository's own dialog: what it holds, one list per section, from its commits to its
//! closed pull requests.

use std::{cell::Cell, fmt::Write, rc::Rc, sync::Arc};

use gitview_core::{
	Client,
	models::{ChangedFile, Commit, CommitDetails, Issue, IssueState},
};
use wx_utils::{
	dialog_padding,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use super::{open_issue, page::show_page};
use crate::{
	config,
	ui::{
		feed::{self, Feed, FeedSection, RowActions},
		live, notice,
		sectioned::SectionedLists,
		text::{self, commit_label, count, local_time},
		web,
	},
};

const LIST_SIZE: Size = Size { width: 600, height: 400 };

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

impl FeedSection for Section {
	fn title(self) -> &'static str {
		match self {
			Self::Commits => "Commits",
			Self::Issues => "Issues",
			Self::PullRequests => "Pull requests",
			Self::ClosedIssues => "Closed issues",
			Self::ClosedPullRequests => "Closed pull requests",
		}
	}

	fn empty_message(self) -> &'static str {
		match self {
			Self::Commits => "No commits.",
			Self::Issues => "No open issues.",
			Self::PullRequests => "No open pull requests.",
			Self::ClosedIssues => "No closed issues.",
			Self::ClosedPullRequests => "No closed pull requests.",
		}
	}
}

impl Section {
	const ALL: [Self; 5] =
		[Self::Commits, Self::Issues, Self::PullRequests, Self::ClosedIssues, Self::ClosedPullRequests];

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
}

/// Shows `repo` (`owner/name`), loading its commits and open issues while the dialog is up, and
/// its closed issues the first time a section of them is chosen. A list stays empty until what
/// it lists arrives.
pub fn open_repository<P: WxWidget + WxEvtHandler + Copy + 'static>(parent: P, client: Arc<Client>, repo: String) {
	let dialog = Dialog::builder(&parent, &repo).build();
	let padding = dialog_padding(&dialog);
	let sections = SectionedLists::new(&dialog, "Sections", &Section::ALL.map(|section| (section, section.title())));
	let lists = config::load().lists;
	for (_, list) in sections.lists() {
		list.set_min_size(LIST_SIZE);
		list.set_start_at_end(lists.oldest_first);
	}
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let commits = Feed::new(
		&sections,
		lists,
		|section| section == Section::Commits,
		|_, commit: &Commit| Some((commit.sha.clone(), commit_label(commit))),
	);
	let open_issues = Feed::new(&sections, lists, |section| section.issue_state() == Some(IssueState::Open), issue_row);
	let closed_issues =
		Feed::new(&sections, lists, |section| section.issue_state() == Some(IssueState::Closed), issue_row);
	// Held weakly, since the lists hold this and the feeds hold the lists. The feeds themselves
	// live as long as this function does, which is as long as the dialog is up.
	let at_end: Rc<dyn Fn(Section)> = {
		let (commits, open_issues, closed_issues) =
			(Rc::downgrade(&commits), Rc::downgrade(&open_issues), Rc::downgrade(&closed_issues));
		let client = Arc::clone(&client);
		Rc::new(move |section| match section.issue_state() {
			None => {
				if let Some(feed) = commits.upgrade() {
					feed::load_more(dialog, &client, &feed, section);
				}
			}
			Some(IssueState::Open) => {
				if let Some(feed) = open_issues.upgrade() {
					feed::load_more(dialog, &client, &feed, section);
				}
			}
			Some(_) => {
				if let Some(feed) = closed_issues.upgrade() {
					feed::load_more(dialog, &client, &feed, section);
				}
			}
		})
	};
	feed::on_reaching_oldest(&sections, lists, &at_end);
	// Speaks what copying did. Made after the lists, so no screen reader takes it for a label.
	let live_region = live::live_region(&dialog);
	feed::on_rows(&sections, live_region, row_actions(dialog, &client, &repo));
	let (commits_client, commits_repo) = (Arc::clone(&client), repo.clone());
	feed::start(
		dialog,
		Arc::clone(&client),
		Rc::clone(&commits),
		move || commits_client.commits(&commits_repo),
		"Could Not Load Commits",
	);
	let (open_client, open_repo) = (Arc::clone(&client), repo.clone());
	feed::start(
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
			feed::start(
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

/// What the dialog's rows do: a commit opens as a page, an issue or pull request as its thread,
/// and each copies its link on GitHub.
fn row_actions(dialog: Dialog, client: &Arc<Client>, repo: &str) -> RowActions<Section> {
	let (client, open_repo) = (Arc::clone(client), repo.to_owned());
	let open = Box::new(move |section, key: String| {
		if section == Section::Commits {
			open_commit(dialog, Arc::clone(&client), open_repo.clone(), key);
		} else if let Ok(number) = key.parse() {
			open_issue(dialog, Arc::clone(&client), open_repo.clone(), number);
		}
	});
	let repo = repo.to_owned();
	let link = Box::new(move |section: Section, key: &str| {
		let kind = match section {
			Section::Commits => "commit",
			_ if section.lists_pull_requests() => "pull",
			_ => "issues",
		};
		format!("https://github.com/{repo}/{kind}/{key}")
	});
	RowActions { open, link }
}

fn issue_row(section: Section, issue: &Issue) -> Option<(String, String)> {
	(issue.is_pull_request() == section.lists_pull_requests()).then(|| (issue.number.to_string(), issue_label(issue)))
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

/// An issue or pull request as its section lists it, without the repository every row shares.
fn issue_label(issue: &Issue) -> String {
	text::issue_label(issue, true)
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
