//! Merging a pull request: checking that GitHub allows it, then asking how, and with what commit
//! title, message and email.

use std::sync::Arc;

use gitview_core::{
	Client,
	models::{Email, MergeCommit, MergeInfo, MergeMethod, PullRequestStatus},
};
use wx_utils::{
	add_ok_cancel_footer, build_ok_cancel_buttons, dialog_padding, show_error, show_warning,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use crate::ui::notice;

const MESSAGE_SIZE: Size = Size { width: 480, height: 120 };
const TITLE: &str = "Merge Pull Request";

/// How the user chose to merge.
struct MergeChoice {
	method: MergeMethod,
	commit: MergeCommit,
	delete_branch: bool,
}

/// Merges pull request `number` of `repo` (`owner/name`), after asking GitHub where it stands and
/// the user how to merge it, then runs `done`. Says why instead when it can't be merged.
pub fn merge_pull_request<P: WxWidget + WxEvtHandler + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	repo: String,
	number: u64,
	done: impl FnOnce() + 'static,
) {
	spawn_for(&parent, async move {
		// Asked afresh, since it may have changed since the thread was opened. The addresses come
		// at the same time; without them, the merge gets GitHub's choice of email.
		let emails = {
			let client = Arc::clone(&client);
			background_blocking(move || client.emails())
		};
		let status = {
			let (client, repo) = (Arc::clone(&client), repo.clone());
			background_blocking(move || client.pull_request_status(&repo, number)).await
		};
		let emails = commit_emails(emails.await.unwrap_or_default());
		let status = match status {
			Ok(status) => status,
			Err(error) => {
				notice::error(&parent, error, "Could Not Merge");
				return;
			}
		};
		if let Some(reason) = why_not(&status, &repo) {
			show_error(&parent, reason, TITLE);
			return;
		}
		let Some(choice) = show_merge_dialog(&parent, number, &status.merge, &emails) else {
			return;
		};
		let merged = {
			let (client, merge, commit) = (Arc::clone(&client), status.merge.clone(), choice.commit);
			background_blocking(move || client.merge_pull_request(&merge, choice.method, &commit)).await
		};
		if let Err(error) = merged {
			notice::error(&parent, error, "Could Not Merge");
			return;
		}
		if choice.delete_branch {
			let branch = status.merge.head_branch.clone();
			let deleted = background_blocking(move || client.delete_branch(&repo, &branch)).await;
			if let Err(error) = deleted {
				show_warning(
					&parent,
					format!("The pull request merged, but its branch couldn't be deleted. {error}"),
					"Branch Not Deleted",
				);
			}
		}
		done();
	});
}

/// The verified addresses a merge commit can be credited to, the primary one first.
fn commit_emails(emails: Vec<Email>) -> Vec<String> {
	let mut verified: Vec<Email> = emails.into_iter().filter(|email| email.verified).collect();
	verified.sort_by_key(|email| !email.primary);
	verified.into_iter().map(|email| email.email).collect()
}

/// Why the pull request can't be merged, or `None` when it can.
fn why_not(status: &PullRequestStatus, repo: &str) -> Option<String> {
	let merge = &status.merge;
	if !merge.open {
		Some("This pull request isn't open any more.".to_owned())
	} else if !merge.allowed {
		Some(format!("Merging into {repo} takes write access, which you don't have."))
	} else if status.draft {
		Some("This pull request is a draft. Mark it ready for review on GitHub before merging it.".to_owned())
	} else if status.conflicts {
		Some("This pull request has conflicts to resolve before it can merge.".to_owned())
	} else if merge.methods.is_empty() {
		Some(format!("{repo} doesn't let pull requests merge."))
	} else {
		None
	}
}

/// The commit title GitHub would give a merge by `method`, which the dialog starts with.
fn default_title(method: MergeMethod, number: u64, merge: &MergeInfo) -> String {
	match method {
		MergeMethod::Merge => format!("Merge pull request #{number} from {}/{}", merge.head_owner, merge.head_branch),
		MergeMethod::Squash | MergeMethod::Rebase => format!("{} (#{number})", merge.title),
	}
}

/// Asks how to merge, or `None` when the user cancels.
fn show_merge_dialog(parent: &dyn WxWidget, number: u64, merge: &MergeInfo, emails: &[String]) -> Option<MergeChoice> {
	let dialog = Dialog::builder(parent, &format!("Merge Pull Request #{number}")).build();
	let padding = dialog_padding(&dialog);
	let methods = merge.methods.clone();
	let method_label = StaticText::builder(&dialog).with_label("Merge &method:").build();
	let method_choice = Choice::builder(&dialog).build();
	for method in &methods {
		method_choice.append(method.label());
	}
	method_choice.set_selection(0);
	let title_label = StaticText::builder(&dialog).with_label("Commit &title:").build();
	let title = TextCtrl::builder(&dialog).with_value(&default_title(methods[0], number, merge)).build();
	let message_label = StaticText::builder(&dialog).with_label("Commit m&essage (blank for GitHub's):").build();
	let message = TextCtrl::builder(&dialog).with_style(TextCtrlStyle::MultiLine).with_size(MESSAGE_SIZE).build();
	// Only worth asking with more than one address to choose from.
	let email = (emails.len() > 1).then(|| {
		let label = StaticText::builder(&dialog).with_label("Commit em&ail:").build();
		let choice = Choice::builder(&dialog).build();
		for address in emails {
			choice.append(address);
		}
		choice.set_selection(0);
		(label, choice)
	});
	let email_choice = email.as_ref().map(|(_, choice)| *choice);
	// A fork's branch isn't the repository's to delete.
	let delete_branch = (!merge.from_fork).then(|| {
		CheckBox::builder(&dialog).with_label(&format!("&Delete the branch {} afterwards", merge.head_branch)).build()
	});
	let (merge_button, cancel_button) = build_ok_cancel_buttons(&dialog, "&Merge");
	{
		// A rebase makes no commit of its own to title. A title the user hasn't changed follows
		// the method, since each has its own default.
		let (methods, merge) = (methods.clone(), merge.clone());
		let shown = std::cell::Cell::new(methods[0]);
		method_choice.on_selection_changed(move |_| {
			let Some(method) = method_choice.get_selection().and_then(|index| methods.get(index as usize)).copied()
			else {
				return;
			};
			if title.get_value() == default_title(shown.get(), number, &merge) {
				title.set_value(&default_title(method, number, &merge));
			}
			shown.set(method);
			title.enable(method.makes_a_commit());
			message.enable(method.makes_a_commit());
			// A rebase keeps each commit's own author.
			if let Some(email_choice) = email_choice {
				email_choice.enable(method.makes_a_commit());
			}
		});
	}
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add_sizer(&row(method_label, &method_choice, padding), 0, SizerFlag::Expand | SizerFlag::All, padding);
	content.add_sizer(
		&row(title_label, &title, padding),
		0,
		SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
		padding,
	);
	content.add(&message_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&message, 1, SizerFlag::Expand | SizerFlag::All, padding);
	if let Some((label, choice)) = email {
		content.add_sizer(
			&row(label, &choice, padding),
			0,
			SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
			padding,
		);
	}
	if let Some(delete_branch) = &delete_branch {
		content.add(delete_branch, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, padding);
	}
	add_ok_cancel_footer(content, merge_button, cancel_button);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	method_choice.set_focus();
	let chosen = (dialog.show_modal() == ID_OK).then(|| {
		let method =
			method_choice.get_selection().and_then(|index| methods.get(index as usize)).copied().unwrap_or(methods[0]);
		let text = |control: TextCtrl| Some(control.get_value().trim().to_owned()).filter(|text| !text.is_empty());
		let chosen_email =
			email_choice.and_then(|choice| choice.get_selection()).and_then(|index| emails.get(index as usize));
		let commit = if method.makes_a_commit() {
			MergeCommit { title: text(title), message: text(message), email: chosen_email.cloned() }
		} else {
			MergeCommit::default()
		};
		MergeChoice {
			method,
			commit,
			delete_branch: delete_branch.is_some_and(|delete_branch| delete_branch.get_value()),
		}
	});
	dialog.destroy();
	chosen
}

/// A label and its control side by side, the control taking the room left.
fn row(label: StaticText, control: &impl WxWidget, padding: i32) -> BoxSizer {
	let row = BoxSizer::builder(Orientation::Horizontal).build();
	row.add(&label, 0, SizerFlag::AlignCenterVertical | SizerFlag::Right, padding);
	row.add(control, 1, SizerFlag::Expand, 0);
	row
}
