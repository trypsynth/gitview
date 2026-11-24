//! A repository's releases: each one's files, which download behind a progress window.

use std::{fmt::Write, path::PathBuf, sync::Arc};

use gitview_core::{Client, models::Release};
use wx_utils::{
	dialog_padding,
	progress::{Ended, run_with_progress},
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use super::{open_on_enter, page::show_page};
use crate::ui::{
	live, notice,
	text::{body_html, count, file_size, local_time},
};

const LIST_SIZE: Size = Size { width: 520, height: 260 };

/// A release as the Releases section lists it, keyed by its ID and its tag: its title, its tag
/// when the title differs, whether it's a pre-release or a draft, when it was published, and how
/// many files it has.
pub fn release_row(release: &Release) -> (String, String) {
	let mut label = release.title().to_owned();
	if release.title() != release.tag_name {
		// Writing to a String can't fail.
		let _ = write!(label, ", tag {}", release.tag_name);
	}
	if release.prerelease {
		label.push_str(", pre-release");
	}
	if release.draft {
		label.push_str(", draft");
	}
	if let Some(published) = &release.published_at {
		let _ = write!(label, ", {}", local_time(published));
	}
	let _ = write!(label, ", {}", count(release.assets.len() as u64, "asset", "assets"));
	(format!("{}/{}", release.id, release.tag_name), label)
}

/// The page of the release a row's key names, in `repo` (`owner/name`).
pub fn release_link(repo: &str, key: &str) -> String {
	let tag = key.split_once('/').map_or(key, |(_, tag)| tag);
	format!("https://github.com/{repo}/releases/tag/{tag}")
}

/// Loads the release a row's key names, in `repo` (`owner/name`), and shows its files.
pub fn open_release(dialog: Dialog, client: Arc<Client>, repo: String, key: &str) {
	let Some(id) = key.split_once('/').and_then(|(id, _)| id.parse::<u64>().ok()) else {
		return;
	};
	let (fetch_client, fetch_repo) = (Arc::clone(&client), repo.clone());
	let fetch = background_blocking(move || fetch_client.release(&fetch_repo, id));
	spawn_for(&dialog, async move {
		match fetch.await {
			Ok(release) => show_release_dialog(&dialog, client, &repo, &release),
			Err(error) => notice::error(&dialog, error, "Could Not Open Release"),
		}
	});
}

/// A file a release offers.
#[derive(Clone)]
struct Download {
	/// What the list says about it.
	label: String,
	/// The name it's saved under unless the user picks another.
	file_name: String,
	/// Its address in GitHub's API.
	url: String,
}

/// The release's own files, then its source code as GitHub archives it.
fn downloads(repo: &str, release: &Release) -> Vec<Download> {
	let mut downloads: Vec<Download> = release
		.assets
		.iter()
		.map(|asset| Download {
			label: format!(
				"{}, {}, {}",
				asset.name,
				file_size(asset.size),
				count(asset.download_count, "download", "downloads")
			),
			file_name: asset.name.clone(),
			url: asset.url.clone(),
		})
		.collect();
	// Named as GitHub names them, from the repository and the tag.
	let name = repo.split_once('/').map_or(repo, |(_, name)| name);
	let stem = format!("{name}-{}", release.tag_name.replace('/', "-"));
	for (label, url, extension) in
		[("Source code (zip)", &release.zipball_url, "zip"), ("Source code (tar.gz)", &release.tarball_url, "tar.gz")]
	{
		if let Some(url) = url {
			downloads.push(Download {
				label: label.to_owned(),
				file_name: format!("{stem}.{extension}"),
				url: url.clone(),
			});
		}
	}
	downloads
}

/// Where downloads go unless the user picks somewhere else: their Downloads folder, when it's
/// there.
fn downloads_folder() -> Option<PathBuf> {
	let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
	Some(PathBuf::from(home).join("Downloads")).filter(|folder| folder.is_dir())
}

fn show_release_dialog(parent: &dyn WxWidget, client: Arc<Client>, repo: &str, release: &Release) {
	let dialog = Dialog::builder(parent, &format!("{repo}: {}", release.title())).build();
	let padding = dialog_padding(&dialog);
	let files = downloads(repo, release);
	let label = StaticText::builder(&dialog)
		.with_label(&format!("&Files. {} to download.", count(files.len() as u64, "file", "files")))
		.build();
	let list = ListBox::builder(&dialog).with_size(LIST_SIZE).build();
	for file in &files {
		list.append(&file.label);
	}
	list.set_selection(0, true);
	// Says how downloads went. Made after the list, so no screen reader takes it for its label.
	let live_region = live::live_region(&dialog);
	let download_button = Button::builder(&dialog).with_label("&Download...").build();
	let notes_button = Button::builder(&dialog).with_label("Release &notes").build();
	let close_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Close").build();
	dialog.set_escape_id(ID_CANCEL);
	let download = move || {
		let Some(file) = list.get_selection().and_then(|index| files.get(index as usize)).cloned() else {
			return;
		};
		download_file(dialog, Arc::clone(&client), &file, live_region);
	};
	open_on_enter(dialog, list, download.clone());
	let double_clicked = download.clone();
	list.on_item_double_clicked(move |_| double_clicked());
	download_button.on_click(move |_| download());
	let (heading, meta, notes) = (
		release.title().to_owned(),
		release.published_at.as_deref().map_or_else(
			|| release.tag_name.clone(),
			|published| format!("{}, published {}", release.tag_name, local_time(published)),
		),
		body_html(release.body_html.as_deref(), release.body.as_deref()),
	);
	notes_button.on_click(move |_| show_page(&dialog, &heading, &meta, &notes));
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	buttons.add(&download_button, 0, SizerFlag::empty(), 0);
	buttons.add(&notes_button, 0, SizerFlag::Left, padding);
	buttons.add_stretch_spacer(1);
	buttons.add(&close_button, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, padding);
	content.add(&list, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, padding);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	list.set_focus();
	dialog.show_modal();
	dialog.destroy();
}

/// Asks where to save `file`, then downloads it there behind a progress window, saying how it
/// went through `live_region`.
fn download_file(dialog: Dialog, client: Arc<Client>, file: &Download, live_region: StaticText) {
	let mut chooser = FileDialog::builder(&dialog)
		.with_message(&format!("Save {}", file.file_name))
		.with_default_file(&file.file_name)
		.with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt);
	if let Some(folder) = downloads_folder() {
		chooser = chooser.with_default_dir(&folder.to_string_lossy());
	}
	let chooser = chooser.build();
	let path = (chooser.show_modal() == ID_OK).then(|| chooser.get_path()).flatten();
	chooser.destroy();
	let Some(path) = path.map(PathBuf::from) else {
		return;
	};
	let name = path.file_name().map_or_else(|| file.file_name.clone(), |name| name.to_string_lossy().into_owned());
	let url = file.url.clone();
	run_with_progress(
		&dialog,
		"Downloading",
		&format!("Downloading {name}..."),
		move |progress| {
			client.download(&url, &path, |done, total| {
				progress.set(done, total);
				!progress.is_cancelled()
			})
		},
		move |result, ended| match (result, ended) {
			(Ok(true), Ended::Completed) => live_region::announce(live_region, &format!("Downloaded {name}.")),
			(Ok(_), _) => live_region::announce(live_region, "Download cancelled."),
			(Err(error), _) => notice::error(&dialog, error, "Could Not Download"),
		},
	);
}
