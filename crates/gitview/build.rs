use std::{env, path::PathBuf};

use shipfitter::{
	build::{configure_file, embed_commit_info, target_profile_dir},
	docs::{Page, convert},
};

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	// The commit this is built from, which the updater's dev channel compares, and whether it's a
	// dev build rather than a tagged release.
	embed_commit_info("GITVIEW");
	shipfitter::windows::embed_manifest("Gitview").expect("failed to embed the Windows manifest");
	// The installer script for `cargo xtask release`, next to the executable it installs, with this
	// build's version and architecture filled in.
	let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("../..");
	let manual = root.join("doc/readme.md");
	println!("cargo:rerun-if-changed={}", manual.display());
	if let Some(target_dir) = target_profile_dir() {
		configure_file(&root.join("gitview.iss.in"), &target_dir.join("gitview.iss"), &[])
			.expect("failed to write the installer script");
		// The manual, as Help opens it and the zip and installer ship it.
		let page = Page { title: "Gitview User Manual", ..Page::default() };
		convert(&manual, &target_dir.join("readme.html"), &page).expect("failed to write the manual");
	}
}
