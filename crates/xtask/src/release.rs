use std::{
	error::Error,
	path::{Path, PathBuf},
};

use shipfitter::{
	host_arch_suffix,
	package::{Zip, cargo_build_release, inno_setup},
	sign::Minisign,
};

const EXE_NAME: &str = if cfg!(windows) { "gitview.exe" } else { "gitview" };

/// Builds Gitview in release mode, then packages it in `target/release`, named for the machine's
/// architecture, such as `x64`: `gitview-x64.zip`, a copy that keeps its settings beside it, and
/// on Windows `gitview_setup-x64.exe`, which installs it. With the minisign key in `MINISIGN_KEY`
/// and its password in `MINISIGN_PASSWORD`, each gets a `.minisig` signature beside it, which the
/// updater checks before installing anything.
pub fn release() -> Result<(), Box<dyn Error>> {
	// The key is read before anything is built, so a broken secret fails at once.
	let minisign = Minisign::from_env()?;
	let artifacts = package()?;
	match minisign {
		Some(minisign) => {
			for artifact in &artifacts {
				println!("Signed {}", minisign.sign(artifact)?.display());
			}
		}
		None => println!("MINISIGN_KEY isn't set, so nothing is signed, and the updater won't install these."),
	}
	Ok(())
}

/// Builds and packages Gitview, returning the files made.
fn package() -> Result<Vec<PathBuf>, Box<dyn Error>> {
	let root = project_root();
	cargo_build_release(&root, &["gitview"])?;
	let target_dir = root.join("target/release");
	let exe = target_dir.join(EXE_NAME);
	if !exe.exists() {
		return Err(format!("{} wasn't built", exe.display()).into());
	}
	let arch = host_arch_suffix();
	let zip_path = target_dir.join(format!("gitview-{arch}.zip"));
	let mut zip = Zip::create(&zip_path)?;
	zip.file(&exe, EXE_NAME)?;
	zip.finish()?;
	println!("Created zip: {}", zip_path.display());
	let mut artifacts = vec![zip_path];
	// The build script wrote the installer script beside the executable. Without Inno Setup this
	// only warns, since the zip is already made.
	if cfg!(windows) && inno_setup(&target_dir.join("gitview.iss"))? {
		let installer = target_dir.join(format!("gitview_setup-{arch}.exe"));
		println!("Created installer: {}", installer.display());
		artifacts.push(installer);
	}
	Ok(artifacts)
}

fn project_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.ancestors()
		.nth(2)
		.expect("xtask lives two levels below the workspace")
		.to_path_buf()
}
