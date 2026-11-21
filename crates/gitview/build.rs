use std::{env, path::PathBuf};

use shipfitter::build::{configure_file, embed_commit_info, target_profile_dir};

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	// The commit this is built from, which the updater's dev channel compares, and whether it's a
	// dev build rather than a tagged release.
	embed_commit_info("GITVIEW");
	shipfitter::windows::embed_manifest("Gitview").expect("failed to embed the Windows manifest");
	// The installer script for `cargo xtask release`, next to the executable it installs, with this
	// build's version and architecture filled in.
	if let Some(target_dir) = target_profile_dir() {
		let template = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("../../gitview.iss.in");
		configure_file(&template, &target_dir.join("gitview.iss"), &[]).expect("failed to write the installer script");
	}
}
