// libopenmpt, from vendor/, for the programs that play tracker modules: Audio and the hub's converter. LAME and
// libFLAC are dotany-sound-encode's.
use std::path::{Path, PathBuf};

fn cpp_files(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "cpp"))
        .collect();
    list.sort();
    list
}

// libopenmpt is compiled straight into the executable, the way its own makefile builds it: the same folders, the same one
// define, standard C++17, so Windows and the Mac compile the same code. Nothing else is switched on: no zlib or miniz, no
// mpg123 or minimp3, no Ogg, Vorbis or stb_vorbis, no Media Foundation, no Opus. vendor/libopenmpt doesn't even carry
// the include/ folder those come from, and audio/scripts/check-deps.mjs fails Audio's build if an MPT_WITH_ is here.
fn build_openmpt() {
    let root = Path::new("vendor/libopenmpt");
    println!("cargo:rerun-if-changed=vendor/libopenmpt");
    let mut cc = cc::Build::new();
    cc.cpp(true)
        .std("c++17")
        .warnings(false)
        .extra_warnings(false)
        .static_crt(false)
        .opt_level_str("s")
        .include(root)
        .include(root.join("src"))
        .include(root.join("common"))
        .define("LIBOPENMPT_BUILD", None)
        .define("_CRT_SECURE_NO_WARNINGS", None);
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // its larger files have more sections than MSVC allows by default; its sources are UTF-8
        cc.flag("/bigobj").flag("/utf-8").flag("/EHsc").flag("/Zc:preprocessor").flag("/Zc:__cplusplus");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // its Windows code expects the wide-character API, as its own Windows builds use
        cc.define("UNICODE", None).define("_UNICODE", None);
    }
    for dir in [
        "src/openmpt/all",
        "src/openmpt/base",
        "src/openmpt/logging",
        "src/openmpt/random",
        "common",
        "src/openmpt/fileformat_base",
        "src/openmpt/soundbase",
        "src/openmpt/soundfile_data",
        "soundlib",
        "soundlib/plugins",
        "soundlib/plugins/dmo",
        "sounddsp",
        "libopenmpt",
    ] {
        for f in cpp_files(&root.join(dir)) {
            cc.file(f);
        }
    }
    cc.compile("dotany_openmpt");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // what its Windows code calls: CoCreateGuid and UuidCreate
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=rpcrt4");
    }
}

fn main() {
    build_openmpt();
}
