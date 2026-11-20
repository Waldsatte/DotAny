//! The family's fonts (Stack Sans Text and Headline, HarmonyOS Sans SC Regular and Bold) are installed once, in the
//! `fonts` folder beside the programs, and every DotAny program reads that one copy.
//! Where they are looked for, in order:
//!
//! 1. `DOTANY_FONTS_DIR`, when it is set, and only there: tests.
//! 2. `fonts` beside the program: DotAny as installed.
//! 3. On the Mac, `DotAny.app/Contents/Resources/fonts`: the shared fonts travel inside DotAny (the hub), and every
//!    tool's `.app` sits beside it in the same folder. DotAny itself finds them in its own `Resources`.
//! 4. `shared/fonts` of the checkout the program was built from: development, and every tool's tests.
//!
//! When none has them the page uses the system's fonts, which its font lists already name after the family's
//! (Microsoft YaHei UI), so a missing folder looks plainer but never breaks a window.
//!
//! Nothing is kept in memory: a font is read from its file each time the page asks for it and dropped as soon as the
//! page has it (the page keeps its own copy anyway, and a second one here would cost every program about 16 MB for
//! its whole life). An installed DotAny doesn't even ask for HarmonyOS Sans: the installer makes it a Windows font and
//! the page takes it from there (`shared/web/fonts.js`). PDF still reads the files to put the fonts in a PDF.
//! Reads come from Windows' file cache, which every DotAny program shares.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The shared fonts, by the name the page asks for.
pub const FILES: [&str; 4] = [
    "HarmonyOS_SansSC_Regular.ttf",
    "HarmonyOS_SansSC_Bold.ttf",
    "StackSansText-wght.ttf",
    "StackSansHeadline-wght.ttf",
];

/// The fonts only one language needs: HarmonyOS Sans TC for Traditional Chinese, Noto Sans JP (cut to
/// common kanji, renamed so it never shadows a full copy) for Japanese, Pretendard for Korean. A page asks for one only
/// when it shows that language.
pub const REGIONAL: [&str; 5] = [
    "HarmonyOS_Sans_TC.ttf",
    "NotoSansJP-DotAny-Regular.ttf",
    "NotoSansJP-DotAny-Bold.ttf",
    "Pretendard-Regular.otf",
    "Pretendard-Bold.otf",
];

/// `shared/fonts` of the checkout this was built from.
const BUILT_FROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fonts");

fn find() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DOTANY_FONTS_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let has = |dir: &Path| dir.join(FILES[0]).is_file();
    let exe = std::env::current_exe().ok();
    if let Some(beside) = exe.as_deref().and_then(|e| e.parent().map(|p| p.join("fonts"))) {
        if has(&beside) {
            return Some(beside);
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(hub) = exe.as_deref().and_then(in_the_hub).filter(|d| has(d)) {
        return Some(hub);
    }
    let built = PathBuf::from(BUILT_FROM);
    has(&built).then_some(built)
}

/// Where the Mac's shared fonts are for a program at `exe`: in `DotAny.app/Contents/Resources/fonts`, DotAny being this
/// program's own `.app` or the one beside it (`…/DotAny/DotAny Text.app/Contents/MacOS/dotany-text` →
/// `…/DotAny/DotAny.app/Contents/Resources/fonts`). None for a program outside an `.app` (a build).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn in_the_hub(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    if !bundle.extension().is_some_and(|e| e == "app") {
        return None;
    }
    Some(bundle.parent()?.join("DotAny.app").join("Contents").join("Resources").join("fonts"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macs_shared_fonts_are_in_dotany_beside_the_tool() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(
            in_the_hub(&p("/Applications/DotAny/DotAny Text.app/Contents/MacOS/dotany-text")),
            Some(p("/Applications/DotAny/DotAny.app/Contents/Resources/fonts"))
        );
        // DotAny itself: its own
        assert_eq!(
            in_the_hub(&p("/Applications/DotAny/DotAny.app/Contents/MacOS/dotany")),
            Some(p("/Applications/DotAny/DotAny.app/Contents/Resources/fonts"))
        );
        // a build, not in an .app
        assert_eq!(in_the_hub(&p("/opt/build/target/debug/dotany-text")), None);
    }
}

/// The folder the fonts come from, if there is one.
pub fn folder() -> Option<&'static Path> {
    static FOLDER: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOLDER.get_or_init(find).as_deref()
}

/// Called first thing in the process: reads Stack Sans through once in the background, in small pieces that are thrown
/// away, so it is in the system's file cache by the time the page asks for it, which it always does at once.
pub fn start_reading() {
    std::thread::spawn(|| {
        use std::io::Read;
        let Some(dir) = folder() else { return };
        let mut piece = vec![0u8; 64 * 1024];
        for name in FILES.iter().filter(|n| n.starts_with("StackSans")) {
            if let Ok(mut file) = std::fs::File::open(dir.join(name)) {
                while matches!(file.read(&mut piece), Ok(n) if n > 0) {}
            }
        }
    });
}

pub fn respond(path: &str) -> tauri::http::Response<Cow<'static, [u8]>> {
    let wanted = path.trim_start_matches('/');
    let kind = if wanted.ends_with(".otf") { "font/otf" } else { "font/ttf" };
    let found = FILES
        .iter()
        .chain(REGIONAL.iter())
        .find(|name| **name == wanted)
        .and_then(|name| std::fs::read(folder()?.join(name)).ok());
    let builder =
        tauri::http::Response::builder().header("Access-Control-Allow-Origin", "*").header("Content-Type", kind);
    match found {
        Some(data) => builder.status(200).body(Cow::Owned(data)).unwrap(),
        None => builder.status(404).body(Cow::Borrowed(&[][..])).unwrap(),
    }
}
