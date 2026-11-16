fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	shipfitter::windows::embed_manifest("Gitview").expect("failed to embed the Windows manifest");
}
