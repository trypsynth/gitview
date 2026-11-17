use std::{cell::Cell, rc::Rc, sync::Arc};

use gitview_core::{
	Client, Error,
	models::{Profile, ProfileUpdate},
};
use wx_utils::{
	confirm, dialog_padding,
	tasks::{background_blocking, spawn_for},
};
use wxdragon::prelude::*;

use crate::ui::{live, notice};

const TITLE: &str = "Edit Profile";
const HIDE_EMAIL: &str = "Don't show my email address";
// GitHub's own limits on the profile settings page.
const BIO_LIMIT: usize = 160;
const SOCIAL_ACCOUNT_LIMIT: usize = 4;
const FIELD_WIDTH: i32 = 350;
const BIO_HEIGHT: i32 = 70;

struct Loaded {
	profile: Profile,
	/// Verified addresses, the only ones GitHub lets a profile show.
	emails: Vec<String>,
	social_accounts: Vec<String>,
}

/// Loads the signed-in user's profile and shows it for editing. `reauthorize` runs when the
/// sign-in cannot edit profiles and the user agrees to sign in again.
pub fn edit_profile<P: WxWidget + WxEvtHandler + Copy + 'static>(
	parent: P,
	client: Arc<Client>,
	reauthorize: impl FnOnce() + 'static,
) {
	spawn_for(&parent, async move {
		match load(Arc::clone(&client)).await {
			Ok(loaded) => show_profile_dialog(&parent, client, loaded),
			Err(Error::NeedsProfileAccess) => {
				if confirm(
					&parent,
					"Editing your profile needs a permission Gitview didn't ask for when you signed in. Sign in again to grant it?",
					TITLE,
				) {
					reauthorize();
				}
			}
			Err(error) => notice::error(&parent, error, "Could Not Load Profile"),
		}
	});
}

async fn load(client: Arc<Client>) -> Result<Loaded, Error> {
	// The profile goes first on its own: it is what tells a sign-in without the scope apart.
	let profile_client = Arc::clone(&client);
	let profile = background_blocking(move || profile_client.profile()).await?;
	// The other two start together, so they travel side by side.
	let emails_client = Arc::clone(&client);
	let emails = background_blocking(move || emails_client.emails());
	let social_accounts = background_blocking(move || client.social_accounts());
	let (emails, social_accounts) = (emails.await?, social_accounts.await?);
	Ok(Loaded {
		profile,
		emails: emails.into_iter().filter(|email| email.verified).map(|email| email.email).collect(),
		social_accounts: social_accounts.into_iter().map(|account| account.url).collect(),
	})
}

fn show_profile_dialog(parent: &dyn WxWidget, client: Arc<Client>, loaded: Loaded) {
	let Loaded { profile, mut emails, social_accounts } = loaded;
	let dialog = Dialog::builder(parent, TITLE).build();
	let padding = dialog_padding(&dialog);
	let field_size = Size { width: dialog.from_dip_int(FIELD_WIDTH), height: -1 };
	let fields = FlexGridSizer::builder(0, 2).with_vgap(padding / 2).with_hgap(padding).build();
	fields.add_growable_col(1, 1);
	// Each label is created before its control: screen readers name a control after the label
	// just ahead of it in creation order.
	let add_label = |label: &str, align: SizerFlag| {
		let label = StaticText::builder(&dialog).with_label(label).build();
		fields.add(&label, 0, align, 0);
	};
	let text_field = |label: &str, value: Option<&str>| {
		add_label(label, SizerFlag::AlignCenterVertical);
		let field = TextCtrl::builder(&dialog).with_value(value.unwrap_or_default()).with_size(field_size).build();
		fields.add(&field, 1, SizerFlag::Expand, 0);
		field
	};
	let name = text_field("&Name:", profile.name.as_deref());
	let current_email = profile.email.clone().unwrap_or_default();
	add_label("Public &email:", SizerFlag::AlignCenterVertical);
	let email = email_choice(dialog, &mut emails, &current_email);
	fields.add(&email, 1, SizerFlag::Expand, 0);
	add_label("&Bio:", SizerFlag::empty());
	let bio = TextCtrl::builder(&dialog)
		.with_value(profile.bio.as_deref().unwrap_or_default())
		.with_style(TextCtrlStyle::MultiLine)
		.with_size(Size { width: field_size.width, height: dialog.from_dip_int(BIO_HEIGHT) })
		.build();
	bio.set_max_length(BIO_LIMIT);
	fields.add(&bio, 1, SizerFlag::Expand, 0);
	let blog = text_field("&URL:", profile.blog.as_deref());
	let socials: [TextCtrl; SOCIAL_ACCOUNT_LIMIT] = std::array::from_fn(|index| {
		text_field(&format!("Social account &{}:", index + 1), social_accounts.get(index).map(String::as_str))
	});
	let company = text_field("&Company:", profile.company.as_deref());
	let location = text_field("&Location:", profile.location.as_deref());
	let hireable = CheckBox::builder(&dialog).with_label("Available for &hire").build();
	hireable.set_value(profile.hireable.unwrap_or(false));
	// Save is a plain button rather than the stock OK, so the dialog stays up while the
	// changes travel and can still be fixed if GitHub turns them down.
	let save_button = Button::builder(&dialog).with_label("&Save").build();
	let cancel_button = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
	// Says the save has started. Made after the buttons, so no screen reader takes it for a label.
	let live_region = live::live_region(&dialog);
	save_button.set_default();
	dialog.set_escape_id(ID_CANCEL);
	// Save stays enabled while a save runs: disabling the button that has focus would send focus
	// to Cancel, which a screen reader announces as if a new dialog had opened.
	let saving = Rc::new(Cell::new(false));
	save_button.on_click(move |_| {
		if saving.replace(true) {
			return;
		}
		live_region::announce(live_region, "Saving profile.");
		// The first choice hides the address, which GitHub takes as an empty one.
		let email = email
			.get_selection()
			.and_then(|index| index.checked_sub(1))
			.and_then(|index| emails.get(index as usize))
			.cloned()
			.unwrap_or_default();
		let update = ProfileUpdate {
			name: name.get_value(),
			email: (email != current_email).then_some(email),
			bio: bio.get_value(),
			blog: blog.get_value().trim().to_owned(),
			company: company.get_value(),
			location: location.get_value(),
			hireable: hireable.get_value(),
		};
		let wanted: Vec<String> =
			socials.iter().map(|field| field.get_value().trim().to_owned()).filter(|url| !url.is_empty()).collect();
		let removed: Vec<String> = social_accounts.iter().filter(|url| !wanted.contains(url)).cloned().collect();
		let added: Vec<String> = wanted.iter().filter(|url| !social_accounts.contains(url)).cloned().collect();
		let client = Arc::clone(&client);
		let saving = Rc::clone(&saving);
		// Tied to the dialog, so a save that finishes after Cancel touches nothing.
		spawn_for(&dialog, async move {
			match background_blocking(move || save(&client, &update, &removed, &added)).await {
				Ok(()) => dialog.end_modal(ID_OK),
				Err(error) => {
					notice::error(&dialog, error, "Could Not Save Profile");
					saving.set(false);
				}
			}
		});
	});
	let button_row = BoxSizer::builder(Orientation::Horizontal).build();
	button_row.add(&save_button, 0, SizerFlag::Right, padding);
	button_row.add(&cancel_button, 0, SizerFlag::empty(), 0);
	let content = BoxSizer::builder(Orientation::Vertical).build();
	content.add_sizer(&fields, 1, SizerFlag::Expand | SizerFlag::All, padding);
	content.add(&hireable, 0, SizerFlag::Left | SizerFlag::Right, padding);
	content.add_sizer(&button_row, 0, SizerFlag::AlignRight | SizerFlag::All, padding);
	dialog.set_sizer_and_fit(content, true);
	dialog.centre();
	name.set_focus();
	dialog.show_modal();
	dialog.destroy();
}

/// The public email choices: hidden, then each verified address. The current address is added
/// if it is missing, so opening the dialog and saving never changes it by accident.
fn email_choice(dialog: Dialog, emails: &mut Vec<String>, current: &str) -> Choice {
	let choice = Choice::builder(&dialog).build();
	choice.append(HIDE_EMAIL);
	if !current.is_empty() && !emails.iter().any(|address| address == current) {
		emails.insert(0, current.to_owned());
	}
	for address in emails.iter() {
		choice.append(address);
	}
	let selected = emails.iter().position(|address| address == current).map_or(0, |index| index + 1);
	choice.set_selection(u32::try_from(selected).unwrap_or_default());
	choice
}

fn save(client: &Client, update: &ProfileUpdate, removed: &[String], added: &[String]) -> Result<(), Error> {
	client.update_profile(update)?;
	// Removals first, so swapping one account for another never goes over GitHub's limit.
	if !removed.is_empty() {
		client.remove_social_accounts(removed)?;
	}
	if !added.is_empty() {
		client.add_social_accounts(added)?;
	}
	Ok(())
}
