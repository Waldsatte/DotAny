// The fonts are packed by dotany-base's own build script, and libarchive's readers are built by dotany-unpack's
// (shared/unpack/build.rs). This one builds the Tauri side.
fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).unwrap();
}
