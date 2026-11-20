# DotAny Archive

DotAny Archive opens an archive, lets you look inside it, change what it holds, extract it and save the result, and
leaves the file alone until you save. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript,
built with esbuild) and a Rust program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs
with Apple silicon.

## What it opens and saves

| Format | Opens | Can be saved as |
|---|---|---|
| ZIP | yes (ZipCrypto and AES passwords) | ZIP (AES-256 password if one is set) |
| 7z | yes (passwords, even with encrypted names) | 7z (AES-256 password if one is set) |
| TAR, TAR.GZ | yes | TAR, TAR.GZ |
| RAR (4 and 5) | yes, read only | ZIP |
| TAR.BZ2, TAR.XZ, TAR.ZST (also `.tbz2` `.txz` `.tzst`) | yes, read only | TAR.GZ |
| a single file in `.gz`, `.bz2`, `.xz` or `.zst` | yes, read only, shown as an archive holding that file | ZIP |
| LZH/LHA, cpio (bare or compressed), ar, `.deb`, `.rpm` | yes, read only | ZIP |

RAR and the read-only formats are never created or written: an edited one is saved as ZIP, and a compressed TAR as
TAR.GZ.

## What you can do with an archive

Browse it as folders and a list, with a path along the top; search across every folder; preview a text file or a
picture (Space); see what it holds, its size and what it saves; check every entry's checksum; extract everything or
only what is picked; add files and folders by choosing them or dropping them on the window; make folders, rename,
delete, undo and redo; start a new, empty archive; save with a compression level (fast, balanced, smallest) and an
optional password; open an entry in the DotAny tool for its kind when that tool is installed beside this one, or in the
system's own program. Names that come out garbled (an old ZIP's, an LHA's) can be read as UTF-8, GBK, Big5 or
Shift-JIS from the info panel. The system's right-click menu can offer the archive's actions, when you turn that on in
settings.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-archive.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is
the family's, `shared/scripts/build.mjs`; this folder's `scripts/build.mjs` adds the icons and the licence notice of
libarchive. `src/notices.generated.js`, the licence notices the program shows, is written by
`shared/scripts/notices.mjs`. The Mac's application and its `Info.plist` and `lproj/` folders are made by
`shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Archive itself (`TOOL`), the archive open in each window and the one job (extract, save, check)
  running on it, and the commands the page calls.
- `pack.rs`: writing a new archive (ZIP, 7z, TAR, TAR.GZ). `extract.rs`: extracting.
- `platform.rs`: opening a file in its usual program, showing a folder, and the Mac's name and file conventions.
- `shellmenu.rs` (Windows) and the module of that name in `lib.rs` (the Mac): the right-click menu.

**Shared with the rest of DotAny:**

- `dotany-unpack` (`../shared/unpack`), which the hub's converter reads archives with too, so a fix there reaches both:
  `source.rs` (the entries of an archive, streamed in the archive's own order), `codec.rs` (the compressed formats) and
  `libarchive.rs` (RAR, LHA, cpio and ar), with libarchive's source in `vendor/`.
- `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files, the watcher that notices when
  another program changes the file, fonts and the platform layer.

**The page** (`src/`):

- `main.js`: the window, the list, the panels, undo, and the extract and save cards. `backend.js`: the calls to the
  Rust side. `lang/`: this tool's words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings, the Save As card, fonts, the
  text caret, keys, sound, and the family's words and licences.

## How it works

**The page holds the archive as a flat list of paths** (folders are entries too) and does all the editing, with one
undo list. Each entry keeps its index in the archive on disk; files added from disk keep their path on disk. Rust
reads and writes; it is sent the list as the page wants the new archive to be.

**Streaming.** Writing or extracting walks the archive once, in its own order, streaming each wanted entry, so a solid
7z or a TAR.GZ is never decoded twice and nothing is held in memory whole. A save goes through
`dotany_base::atomic::atomic_produce` into a temporary file beside the target.

**Compressed files** (`codec.rs`). gzip, bzip2, xz and zstd are known by their first bytes, not their names, and read
as a stream by pure-Rust decoders (flate2, bzip2, lzma-rust2, ruzstd). What is inside decides the rest: a TAR, a cpio,
or one plain file, shown as an archive holding just that file, named after the archive without its suffix
(`server.log.gz` holds `server.log`; a `.tgz` holds a `.tar`). Its size is learnt by reading it to its end when it
opens, so a damaged or cut stream is refused there. Extracting one file puts it beside the archive, not in a folder of
its own name.

**Names.** ZIP names marked UTF-8, and 7z and RAR names, are text. Any other raw name bytes go to the page, which
guesses the encoding (UTF-8, GBK, Big5, Shift-JIS) and lets the info panel change it. Saving writes UTF-8: ZIP's
UTF-8 flag, and PAX records in TAR, so Windows' own tar reads them too.

**RAR, LHA, cpio and ar** (`libarchive.rs`). libarchive 3.8.11, in `shared/unpack/vendor/libarchive`, is built by that
crate's `build.rs` with a hand-written `vendor/libarchive-config.h` and only the reading core and those readers, with no
compression library: RAR and LHA decode themselves, and a compressed cpio (an initramfs) or an RPM's payload is
decompressed in Rust and handed to libarchive as a stream. An RPM's lead and headers are skipped in Rust (`source.rs`);
its files are the payload's. A `.deb` shows its three members (`debian-binary`, `control.tar.*`, `data.tar.*`), and
ar's symbol tables are left out. LHA, cpio and ar names are the archive's bytes, guessed by the page like an old ZIP's.
libarchive cannot decrypt RAR, so encrypted RAR entries say so.

## What it is careful about

- **Extracting never writes outside the folder you chose.** A `..` is never followed; drive letters, colons and
  Windows' reserved names are made harmless; every top-level item gets a new name when its name is taken, so nothing is
  overwritten; symbolic links are made last and kept only when they lead inside the target. A cancel takes back what
  was written. One folder at the top of an archive is not wrapped in another.
- **Damaged data is told apart from the rest.** Every 7z entry is held to its recorded size and checksum (a wrong
  password can otherwise end a stream early), and an entry whose data or checksum is wrong is reported and its partial
  file removed.
- **Saving.** The archive is written to a temporary file in the same folder and swapped in, so a failed save leaves the
  old file whole; the edits stay and the window says why.
- **Programs are not run.** Opening an entry writes it to a temporary folder first, and a program or script (on
  Windows `.exe`, `.bat`, `.msi`, `.lnk`, disk images, macro-enabled Office documents and the like; on the Mac its
  installers, scripts and application bundles) is refused unless a DotAny tool is the one to open it.
- **What comes from the internet stays marked.** Every file extracted or opened from an archive that carries Windows'
  Mark of the Web (`Zone.Identifier`) or the Mac's `com.apple.quarantine` gets the same mark, as Explorer and Archive
  Utility do, so SmartScreen, Office's Protected View and Gatekeeper still look at it.
- **Long jobs report from where they run.** Each window has its own archive and its own job; a job reports its progress
  to its window only, and stops when its window closes.

## Windows and the Mac

- **Windows.** Each archive opens in a program of its own, with one window. The window has no system title bar: the
  page draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button. The right-click menu (`shellmenu.rs`) writes only
  `HKCU\Software\Classes\SystemFileAssociations\.<ext>\shell\DotAnyArchive.Open` and
  `HKCU\Software\Classes\{*,Directory}\shell\DotAnyArchive.Pack`, and removes exactly those; Windows 11 shows them
  under "Show more options". `--pack <path>` starts a new archive holding that file or folder.
- **The Mac.** One program shows each archive in a window of its own. The right-click menu is two Finder Quick Actions:
  one opens archives with DotAny Archive, the other compresses any file or folder with it. Names are shown and written
  composed (NFC), since the
  Mac's disks and libarchive's RAR reader hand them over decomposed; the Finder's `.DS_Store`, `._` files and a zip's
  `__MACOSX/` are never listed, extracted or packed. Unix permissions are kept on extraction, so a program stays
  runnable and a ZIP's or 7z's symbolic links are links again.
