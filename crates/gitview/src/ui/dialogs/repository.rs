//! A repository's own dialog: what it holds, one list per section, from its commits to its
//! closed pull requests.

use std::{
	cell::Cell,
	fmt::Write,
	rc::{Rc, Weak},
	sync::Arc,
};

use gitview_core::{
	Client,
	models::{ChangedFile, Commit, CommitDetails, Issue, IssueState},
};
use quiet_list::{QuietList, Select};
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

/// A section that lists issues, with its list.
type IssueList = (Section, Rc<QuietList<String>>);

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
	bind_lists(dialog, &sections, &client, &repo);
	load_commits(dialog, Rc::clone(sections.list(Section::Commits)), Arc::clone(&client), repo.clone());
	load_issues(dialog, issue_lists(&sections, IssueState::Open), Arc::clone(&client), repo.clone(), IssueState::Open);
	// Closed issues can run to hundreds, so they wait until someone looks.
	let closed_requested = Cell::new(false);
	let closed_lists = issue_lists(&sections, IssueState::Closed);
	sections.on_section_changed(move |section| {
		if section.issue_state() == Some(IssueState::Closed) && !closed_requested.replace(true) {
			load_issues(dialog, closed_lists.clone(), Arc::clone(&client), repo.clone(), IssueState::Closed);
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
/// request as its thread. The lists' own callbacks hold them weakly, so they can go once the
/// dialog closes.
fn bind_lists(dialog: Dialog, sections: &SectionedLists<Section>, client: &Arc<Client>, repo: &str) {
	for (section, list) in sections.lists() {
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

fn load_commits(dialog: Dialog, list: Rc<QuietList<String>>, client: Arc<Client>, repo: String) {
	let fetch = background_blocking(move || client.commits(&repo));
	spawn_for(&dialog, async move {
		match fetch.await {
			Ok(commits) => {
				let rows = commits.iter().map(|commit| (commit.sha.clone(), commit_label(commit))).collect();
				fill(&list, rows, Section::Commits.empty_message());
			}
			Err(error) => {
				fill(&list, Vec::new(), "Could not load commits.");
				notice::error(&dialog, error, "Could Not Load Commits");
			}
		}
	});
}

/// The sections that list issues in `state`, with their lists.
fn issue_lists(sections: &SectionedLists<Section>, state: IssueState) -> Vec<IssueList> {
	sections
		.lists()
		.filter(|(section, _)| section.issue_state() == Some(state))
		.map(|(section, list)| (section, Rc::clone(list)))
		.collect()
}

/// Fetches the issues in `state` and fills `lists`, the sections that show them. GitHub lists
/// issues and pull requests together, so one request fills both.
fn load_issues(dialog: Dialog, lists: Vec<IssueList>, client: Arc<Client>, repo: String, state: IssueState) {
	let fetch = background_blocking(move || client.issues(&repo, state));
	spawn_for(&dialog, async move {
		match fetch.await {
			Ok(issues) => {
				for (section, list) in &lists {
					let rows = issues
						.iter()
						.filter(|issue| issue.is_pull_request() == section.lists_pull_requests())
						.map(|issue| (issue.number.to_string(), issue_label(issue)))
						.collect();
					fill(list, rows, section.empty_message());
				}
			}
			Err(error) => {
				for (section, list) in &lists {
					fill(list, Vec::new(), &format!("Could not load {}.", section.title().to_lowercase()));
				}
				notice::error(&dialog, error, "Could Not Load Issues");
			}
		}
	});
}

/// Puts `rows` in `list`, or `message` alone when there are none.
fn fill(list: &QuietList<String>, rows: Vec<(String, String)>, message: &str) {
	let rows = if rows.is_empty() { vec![(MESSAGE_KEY.to_owned(), message.to_owned())] } else { rows };
	list.update_labelled(rows, Select::Start);
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
