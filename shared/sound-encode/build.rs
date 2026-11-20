// LAME and libFLAC, from vendor/, for every DotAny program that writes MP3 or FLAC: Audio, Recorder and the hub's
// converter. Recorder depends on this crate, Audio and the converter through dotany-sound.
use std::path::{Path, PathBuf};

fn c_files(dir: &Path, skip: &[&str]) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "c"))
        .filter(|p| !skip.contains(&p.file_name().unwrap().to_str().unwrap()))
        .collect();
    list.sort();
    list
}

// The folder the exe lands in: OUT_DIR is <target>/<profile>/build/<crate>-<hash>/out. This script runs for the program
// being built, in that program's target folder, so <target>/<profile> is where its exe lands.
fn exe_folder(out_dir: &str) -> PathBuf {
    Path::new(out_dir).ancestors().nth(3).unwrap().to_path_buf()
}

// LAME is LGPL. It is built as its own DLL beside the exe, loaded at run time, so anyone can swap in their own build of it.
// Only the encoder is built: vendor/lame-3.100 does not contain the decoder (mpglib) at all, and HAVE_MPGLIB is never defined.
fn build_lame(out_dir: &str) {
    let lame = Path::new("vendor/lame-3.100");
    println!("cargo:rerun-if-changed=vendor/lame-3.100");
    println!("cargo:rerun-if-changed=lame.def");
    let include_msvc = Path::new(out_dir).join("lame-config");
    std::fs::create_dir_all(&include_msvc).unwrap();
    std::fs::copy(lame.join("configMS.h"), include_msvc.join("config.h")).unwrap();
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .extra_warnings(false)
        .static_crt(false)
        .opt_level(2)
        .include(lame.join("include"))
        .include(&include_msvc)
        .include(lame.join("libmp3lame"))
        .define("TAKEHIRO_IEEE754_HACK", None)
        .define("FLOAT8", Some("float"))
        .define("REAL_IS_FLOAT", Some("1"))
        .define("BS_FORMAT", Some("BINARY"))
        .define("HAVE_CONFIG_H", None)
        .define("_CRT_SECURE_NO_WARNINGS", None);
    for f in c_files(&lame.join("libmp3lame"), &[]) {
        cc.file(f);
    }
    cc.file(lame.join("libmp3lame/vector/xmm_quantize_sub.c"));
    let objects = cc.compile_intermediates();
    let target = std::env::var("TARGET").unwrap();
    let mut link = cc::windows_registry::find(&target, "link.exe").expect("link.exe from the MSVC tools");
    let dll = Path::new(out_dir).join("libmp3lame.dll");
    link.arg("/NOLOGO")
        .arg("/DLL")
        .arg("/DEF:lame.def")
        .arg(format!("/OUT:{}", dll.display()))
        .arg(format!("/IMPLIB:{}", Path::new(out_dir).join("libmp3lame.lib").display()));
    link.args(&objects);
    let status = link.status().expect("run link.exe");
    assert!(status.success(), "linking libmp3lame.dll failed");
    std::fs::copy(&dll, exe_folder(out_dir).join("libmp3lame.dll")).unwrap();
}

// libFLAC is BSD, so it goes straight into the exe. Only its encoding side is reached; no Ogg.
fn build_flac(out_dir: &str) {
    let flac = Path::new("vendor/flac");
    println!("cargo:rerun-if-changed=vendor/flac");
    let config = Path::new(out_dir).join("flac-config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.h"), "#define CPU_IS_BIG_ENDIAN 0\n#define CPU_IS_LITTLE_ENDIAN 1\n#define ENABLE_64_BIT_WORDS 1\n#define FLAC__HAS_OGG 0\n#define FLAC__HAS_X86INTRIN 1\n#define FLAC__HAS_NEONINTRIN 0\n#define FLAC__HAS_A64NEONINTRIN 0\n#define HAVE_LROUND 1\n#define HAVE_STDINT_H 1\n#define HAVE_INTTYPES_H 1\n#define PACKAGE_VERSION \"1.5.0\"\n#define VERSION \"1.5.0\"\n").unwrap();
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .extra_warnings(false)
        .static_crt(false)
        .opt_level(2)
        .include(flac.join("include"))
        .include(flac.join("src/libFLAC/include"))
        .include(&config)
        .define("HAVE_CONFIG_H", None)
        .define("FLAC__NO_DLL", None)
        .define("_CRT_SECURE_NO_WARNINGS", None)
        .define("_CRT_NONSTDC_NO_DEPRECATE", None);
    for f in c_files(
        &flac.join("src/libFLAC"),
        &["ogg_decoder_aspect.c", "ogg_encoder_aspect.c", "ogg_helper.c", "ogg_mapping.c", "lpc_intrin_neon.c"],
    ) {
        cc.file(f);
    }
    // how it opens files with non-English names on Windows; its encoder and decoder both refer to this
    cc.file(flac.join("src/share/win_utf8_io/win_utf8_io.c"));
    cc.compile("dotany_flac");
}

// ---------- the Mac ----------
// The same sources and DotAny's same switches, with each library's own configuration for an Apple-silicon Mac (macos/),
// in place of the Windows one above.

// A file copied over one a running program has mapped would be killed by the Mac's code signing: the old one goes first.
fn put(from: &Path, to: &Path) {
    let _ = std::fs::remove_file(to);
    std::fs::copy(from, to).unwrap();
}

// LAME stays its own library, as the LGPL asks: libmp3lame.dylib beside the program (in an .app, Contents/Frameworks;
// src/lame.rs looks in both), exporting only what lame.def names, as the Windows DLL does.
fn build_lame_macos(out_dir: &str) {
    let lame = Path::new("vendor/lame-3.100");
    println!("cargo:rerun-if-changed=vendor/lame-3.100");
    println!("cargo:rerun-if-changed=lame.def");
    println!("cargo:rerun-if-changed=macos/lame-config.h");
    let include = Path::new(out_dir).join("lame-config");
    std::fs::create_dir_all(&include).unwrap();
    std::fs::copy("macos/lame-config.h", include.join("config.h")).unwrap();
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .extra_warnings(false)
        .opt_level(2)
        .pic(true)
        .include(lame.join("include"))
        .include(&include)
        .include(lame.join("libmp3lame"))
        .define("TAKEHIRO_IEEE754_HACK", None)
        .define("FLOAT8", Some("float"))
        .define("REAL_IS_FLOAT", Some("1"))
        .define("BS_FORMAT", Some("BINARY"))
        .define("HAVE_CONFIG_H", None);
    // no vector/xmm_quantize_sub.c: it is SSE only, and empty without it
    for f in c_files(&lame.join("libmp3lame"), &[]) {
        cc.file(f);
    }
    let objects = cc.compile_intermediates();
    // lame.def's names, as the Mac's linker spells C names
    let def = std::fs::read_to_string("lame.def").unwrap();
    let names: String = def
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != "EXPORTS")
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|n| format!("_{n}\n"))
        .collect();
    let exports = Path::new(out_dir).join("libmp3lame.exports");
    std::fs::write(&exports, names).unwrap();
    let dylib = Path::new(out_dir).join("libmp3lame.dylib");
    let mut link = cc.get_compiler().to_command();
    link.arg("-dynamiclib")
        .arg("-o")
        .arg(&dylib)
        .arg("-install_name")
        .arg("@rpath/libmp3lame.dylib")
        .arg("-current_version")
        .arg("3.100")
        .arg(format!("-Wl,-exported_symbols_list,{}", exports.display()))
        .args(&objects);
    let status = link.status().expect("run the C compiler to link");
    assert!(status.success(), "linking libmp3lame.dylib failed");
    put(&dylib, &exe_folder(out_dir).join("libmp3lame.dylib"));
}

// libFLAC goes into the program, as on Windows; on ARM its NEON code is built in place of the x86 one.
fn build_flac_macos(out_dir: &str) {
    let flac = Path::new("vendor/flac");
    println!("cargo:rerun-if-changed=vendor/flac");
    println!("cargo:rerun-if-changed=macos/flac-config.h");
    let config = Path::new(out_dir).join("flac-config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::copy("macos/flac-config.h", config.join("config.h")).unwrap();
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .extra_warnings(false)
        .opt_level(2)
        .include(flac.join("include"))
        .include(flac.join("src/libFLAC/include"))
        .include(&config)
        .define("HAVE_CONFIG_H", None)
        .define("FLAC__NO_DLL", None);
    for f in c_files(
        &flac.join("src/libFLAC"),
        &["ogg_decoder_aspect.c", "ogg_encoder_aspect.c", "ogg_helper.c", "ogg_mapping.c"],
    ) {
        cc.file(f);
    }
    cc.compile("dotany_flac");
}

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_lame_macos(&out_dir);
        build_flac_macos(&out_dir);
        return;
    }
    build_lame(&out_dir);
    build_flac(&out_dir);
}
