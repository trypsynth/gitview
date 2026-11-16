//! A repository's own dialog: what it holds, one list per section, starting with its commits.

use std::{
	cell::RefCell,
	fmt::Write,
	rc::{Rc, Weak},
	sync::Arc,
};

use gitview_core::{
	Client,
	models::{ChangedFile, Commit, CommitDetails},
};
use quiet_list::{QuietList, Select};
use wx_utils::{
	dialog_padding, show_error,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use super::{is_plain_enter, page::show_page};
use crate::ui::{
	sectioned::SectionedLists,
	text::{count, local_time},
	web,
};

const LIST_SIZE: Size = Size { width: 600, height: 400 };
/// The key of the row shown in place of commits. It is always the only row, so it cannot be
/// mistaken for a commit's hash.
const MESSAGE_KEY: &str = "";

/// What a repository's dialog lists, in the order its Sections list shows them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
	Commits,
}

impl Section {
	const ALL: [Self; 1] = [Self::Commits];

	const fn title(self) -> &'static str {
		match self {
			Self::Commits => "Commits",
		}
	}
}

/// Shows `repo` (`owner/name`), loading its commits while the dialog is up. The list stays empty
/// until they arrive.
pub fn open_repository<P: WxWidget + WxEvtHandler + Copy + 'static>(parent: P, client: Arc<Client>, repo: String) {
	let dialog = Dialog::builder(&parent, &repo).build();
	let padding = dialog_padding(&dialog);
	let sections = SectionedLists::new(&dialog, "Sections", &Section::ALL.map(|section| (section, section.title())));
	let list = Rc::clone(sections.list(Section::Commits));
	list.set_min_size(LIST_SIZE);
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let commits: Rc<RefCell<Vec<Commit>>> = Rc::default();
	bind_commit_list(dialog, &list, &commits, &client, &repo);
	let fetch = background_blocking(move || client.commits(&repo));
	let fetched_list = Rc::clone(&list);
	spawn_for(&dialog, async move {
		match fetch.await {
			Ok(fetched) => {
				let rows = if fetched.is_empty() {
					vec![(MESSAGE_KEY.to_owned(), "No commits.".to_owned())]
				} else {
					fetched.iter().map(|commit| (commit.sha.clone(), commit_label(commit))).collect()
				};
				*commits.borrow_mut() = fetched;
				fetched_list.update_labelled(rows, Select::Start);
			}
			Err(error) => {
				fetched_list.update_labelled(
					vec![(MESSAGE_KEY.to_owned(), "Could not load commits.".to_owned())],
					Select::Start,
				);
				show_error(&dialog, error, "Could Not Load Commits");
			}
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
	list.set_focus();
	dialog.show_modal();
	dialog.destroy();
}

/// Opens the selected commit on Enter or a double-click. The list's own callbacks hold it weakly,
/// so the list and the dialog can go once the dialog closes.
fn bind_commit_list(
	dialog: Dialog,
	list: &Rc<QuietList<String>>,
	commits: &Rc<RefCell<Vec<Commit>>>,
	client: &Arc<Client>,
	repo: &str,
) {
	let open = {
		let (list, commits, client, repo) =
			(Rc::downgrade(list), Rc::clone(commits), Arc::clone(client), repo.to_owned());
		Rc::new(move || {
			let sha = Weak::upgrade(&list).and_then(|list| list.selected());
			// The message row has no commit, so it opens nothing.
			if let Some(sha) = sha.filter(|sha| commits.borrow().iter().any(|commit| commit.sha == *sha)) {
				open_commit(dialog, Arc::clone(&client), repo.clone(), sha);
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
			Err(error) => show_error(&dialog, error, "Could Not Open Commit"),
		}
	});
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
