use std::env;

use embed_manifest::{
	embed_manifest,
	manifest::{
		ActiveCodePage, DpiAwareness, Setting,
		SupportedOS::{Windows7, Windows10},
	},
	new_manifest,
};

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	if env::var("CARGO_CFG_WINDOWS").is_ok() {
		let manifest = new_manifest("Gitview")
			.supported_os(Windows7..=Windows10)
			.active_code_page(ActiveCodePage::Utf8)
			.dpi_awareness(DpiAwareness::PerMonitorV2)
			.long_path_aware(Setting::Enabled);
		embed_manifest(manifest).expect("failed to embed the Windows manifest");
	}
}
