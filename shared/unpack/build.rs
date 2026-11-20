// Builds libarchive's RAR, LHA, cpio and ar readers from vendor/, once, for every program that reads archives through
// this crate (DotAny Archive, the hub's converter). These are only ever read, never written; nothing else of libarchive
// is used, and none of its compression filters: what wraps a cpio or an RPM's payload is unwrapped in Rust
// (src/codec.rs).
use std::path::Path;

// Only what the readers need: the reading core, strings and entries, RAR 4's PPMd and RAR 5's BLAKE2, and the readers.
const LIBARCHIVE: &[&str] = &[
    "archive_acl.c",
    "archive_blake2s_ref.c",
    "archive_crc32.c",
    "archive_time.c",
    "archive_blake2sp_ref.c",
    "archive_check_magic.c",
    "archive_entry.c",
    "archive_entry_sparse.c",
    "archive_entry_xattr.c",
    "archive_ppmd7.c",
    "archive_read.c",
    "archive_read_open_filename.c",
    "archive_read_set_options.c",
    "archive_options.c",
    "archive_read_support_format_rar.c",
    "archive_read_support_format_rar5.c",
    "archive_read_support_format_lha.c",
    "archive_read_support_format_cpio.c",
    "archive_read_support_format_ar.c",
    "archive_string.c",
    "archive_string_sprintf.c",
    "archive_util.c",
    "archive_virtual.c",
    "archive_windows.c",
    "archive_random.c",
];

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos();
        return;
    }
    let src = Path::new("vendor/libarchive");
    println!("cargo:rerun-if-changed=vendor/libarchive");
    println!("cargo:rerun-if-changed=vendor/libarchive-config.h");
    let config = std::path::absolute("vendor/libarchive-config.h").unwrap();
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .extra_warnings(false)
        .static_crt(false)
        .opt_level(2)
        .include(src)
        .define("PLATFORM_CONFIG_H", Some(format!("\"{}\"", config.display().to_string().replace('\\', "/")).as_str()))
        .define("LIBARCHIVE_STATIC", None)
        .define("_CRT_SECURE_NO_WARNINGS", None)
        .define("UNICODE", None)
        .define("_UNICODE", None);
    for f in LIBARCHIVE {
        cc.file(src.join(f));
    }
    // linked into whichever program uses this crate, once: src/libarchive.rs names it
    cc.compile("dotany_libarchive");
    // the Windows pieces libarchive's core calls: random numbers and the crypto provider
    println!("cargo:rustc-link-lib=advapi32");
    println!("cargo:rustc-link-lib=bcrypt");
}

// The Mac: the same files and readers with the configuration libarchive's own build finds on an Apple-silicon Mac
// (macos/libarchive-config.h). archive_windows.c is Windows' alone, and nothing is linked: arc4random_buf, which the
// core takes its random numbers from there, is in the C library.
fn build_macos() {
    let src = Path::new("vendor/libarchive");
    println!("cargo:rerun-if-changed=vendor/libarchive");
    println!("cargo:rerun-if-changed=macos/libarchive-config.h");
    let config = std::path::absolute("macos/libarchive-config.h").unwrap();
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .extra_warnings(false)
        .opt_level(2)
        .include(src)
        .define("PLATFORM_CONFIG_H", Some(format!("\"{}\"", config.display()).as_str()))
        .define("LIBARCHIVE_STATIC", None);
    for f in LIBARCHIVE.iter().filter(|f| **f != "archive_windows.c") {
        cc.file(src.join(f));
    }
    cc.compile("dotany_libarchive");
}
