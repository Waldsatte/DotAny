// The fonts are packed by dotany-base's own build script; LAME and libFLAC by dotany-sound-encode's (shared/sound-encode),
// which puts libmp3lame.dll beside the exe, and libopenmpt by dotany-sound's (shared/sound). This one only builds the
// Tauri side.
fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).unwrap();
}
