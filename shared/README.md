# DotAny shared base

What every DotAny program is built on. A program is a [Tauri 2](https://tauri.app) application: a page (plain
JavaScript, bundled with esbuild) and a Rust program. The base gives both sides the same window, the same way of opening
and saving a file, the same settings and the same words, so a new tool only writes what is its own: its work area, its
file reading and writing, and its save formats. `starter/` is the smallest complete tool; copy it to begin a new one.

This folder holds the base itself (`rust/`, `web/`, `scripts/`, `fonts/`, `starter/`) and the crates that more than one
program builds from (`table/`, `picture/`, `words/`, `sound/`, `sound-encode/`, `unpack/`, `repack/`, `convert/`).

## What is here

| Folder | What it is |
|---|---|
| `rust/` | `dotany-base`, a path crate. Atomic saves, settings and recent files, the file watcher, the family's fonts, the files beside the open one in natural order, the platform layer for Windows and macOS, the window with its frame and menus, and the commands every page calls. |
| `web/` | `@dotany/web`, the page side, imported by a program's esbuild through an alias. `shell.js` (`createShell`) makes the family's window around a tool's work, from the parts in `shell/`; `family.css` is the family's stylesheet; the other modules are listed below. `lang/` holds the family's words in each language. |
| `scripts/` | The page build (`build.mjs`), the licence notices (`notices.mjs`), the Mac's `Info.plist` files (`mac-plist.mjs`), the throwaway `.app` that runs a build on the Mac and the Mac's release packaging (`mac-app.mjs`, `mac-release.mjs`), and the script that cuts the Japanese font (`subset-jp.py`). |
| `fonts/` | The one copy of the family's fonts (HarmonyOS Sans SC and TC, Stack Sans, Noto Sans JP cut to common kanji, Pretendard) with their licences. |
| `starter/` | DotAny Starter: the family's window around an empty work area. |
| `table/` | `dotany-table`: tables as Sheets reads and writes them (CSV and TSV encodings, the XLSX package, XLS and ODS read through calamine). Sheets and the hub's converter build from it. |
| `picture/` | `dotany-picture`: what a picture file really is, its colour profile, EXIF and XMP, turning, cropping, the sliders, drawing and resizing, and the PCX and IFF ILBM readers. Image and the converter build from it. |
| `words/` | `dotany-words`: Word files as Text reads and writes them (.docx into the editor's blocks and back; doc, rtf and odt read). Text and the converter build from it. |
| `sound/` | `dotany-sound`: sound as Audio reads and writes it (what a file is by its bytes, AU, VOC and G.711 WAV, tracker modules through libopenmpt, each format's writer, sample rates, tags). Audio and the converter build from it. |
| `sound-encode/` | `dotany-sound-encode`: MP3 through LAME, built as its own `libmp3lame` library that the build puts beside the program, and FLAC through libFLAC. Recorder builds from it; Audio and the converter through `sound/`. |
| `unpack/` | `dotany-unpack`: archives as Archive reads them (ZIP, 7z, TAR, gzip, bzip2, xz, zstd, and RAR, LHA, cpio and ar through libarchive's readers). Reading only. Archive and the converter build from it. |
| `repack/` | `dotany-repack`: MKV and AVI films repackaged as MP4 without decoding, for the Mac's player. Video and Slides use it on the Mac. |
| `convert/` | `dotany-convert`: the hub's converter, a library with its own README. It builds from the crates above, so a fix to a format reaches the tool and the converter together. |
| `upstream-licences/` | Licence text of a package whose files are copied into generated output (Lucide's icons). |

What a tool keeps for itself: its work area, its own top-bar buttons after the separator, its side panels and dock, its
file reading and writing, its save formats and their options, and its own words (`src/lang/<code>.json`).

## What the base gives a program

### On the Rust side (`dotany-base`)

- **The window.** `builder` (or `builder_per_window`, or `builder_with` for a program that opens its own windows) returns
  a `tauri::Builder` with the family already in it: the font address, the shared state, the question before closing, the
  frame's state, and the first window. The window has no system title bar on Windows (the page's top bar is the only bar,
  and the page draws the window's buttons); on the Mac it keeps the system's frame (see "The Mac").
- **The commands every page calls**, listed once by `dotany_base::handler![…]`: `set_setting`, `forget_recent`,
  `can_open`, `pick_open`, `pick_save`, `default_folder`, `neighbour`, `close_document`, `keep_mine`, `set_frame`,
  `window_control`, `window_menu`, `set_snap_button`, `set_title`, `window_ready`, `startup_done`, `close_window`,
  `print_window`, `set_menu_words`, `set_edited`. A tool adds its own after them. Commands that concern a file take the
  window that asked; Tauri hands it over and the page never says which.
- **Atomic saves** (`atomic`). `atomic_write` writes a temporary file in the same folder (its name starts with
  `.~dotany-`, which the watcher ignores), flushes it to disk and swaps it for the target, so the target is always the
  old file or the new one. A shortcut or link is followed and the real file replaced. Any failure leaves the original
  as it was and returns a `WriteFailure` (`readonly`, `locked`, `diskfull`, `denied`, `missing`, `other`) that the page
  words for the person. `atomic_produce` is the same for a file too large to hold in memory: the tool writes the temporary
  file itself. On Windows the swap keeps the old file's attributes and streams (`ReplaceFile`); on the Mac it keeps
  permissions, owner, access list, Finder tags and the creation date, and the bytes go down with `F_FULLFSYNC` first.
- **Settings and recent files** (`settings`, `family`). One small `settings.json` per tool, in `%APPDATA%\DotAny\<folder>`
  (on the Mac `~/Library/Application Support/DotAny/<folder>`); a broken file is set aside as `settings.broken.json`,
  never overwritten. The settings every program follows, and one list of recent files, are shared (below).
- **The open file, per window** (`files`). `opened_in` and `saved_in` tell the base what a window opened and wrote; it
  watches the file (`watcher`, which looks at the folder because many programs save by replacing the file), says
  `file-changed` to that window's page when another program changes it, keeps the recent lists, and lets the tool ask
  whether the file is still as it was read (`Base::unchanged_since_read_in`).
- **Files beside the open one** (`folder`): the next and previous file in the folder, in the order people read names
  (2 before 10, case ignored). The `neighbour` command uses it.
- **Fonts** (`fonts`): the family's fonts are found, not built in, and answered to the page one file at a time from the
  `dotfont` address; nothing is kept in memory.
- **A guard against foreign lengths** (`limits`): a count or size read from a file is held to what the file really has
  before memory is asked for it. A parser for a foreign format goes through it.
- **A written-down panic** (`crash`): a panic is appended to `crash.log` in the tool's settings folder (time, program and
  version, message, place, backtrace; the newest 256 KB are kept) and then goes on as it would have. Nothing is shown
  and nothing leaves the computer. `locks` is described under "Panics and locks".
- **The family's one WebView** (`webview`, and `platform/windows/webview.rs`): on Windows every program starts its
  WebView2 in one folder with the same arguments, so a second program joins the first one's browser. If the browser or a
  page ends, the window comes back where it was with its file. A window in the background (minimised, hidden, or behind
  others for a while) puts its page at WebView2's low memory level unless it is playing sound or the tool says it is busy
  (`app.quiet` on the page).
- **Links** (`links`): a web address a file names is opened in the system's browser only when it is plain `http` or
  `https`.
- **Pictures beside Markdown** (`beside`): writing a Word document's pictures next to its Markdown file and linking them.
- **The platform layer** (`platform`): everything that differs between systems lives under it and nothing outside it
  names one. It gives file operations (`create_temp`, `replace_file`, `place_file`, `is_read_only`, `lock_file`,
  `write_failure`, `same_name`), per-user folders, the window's frame, the menu bar and the program's life, the system's
  browser, and the way one tool starts another that is installed beside it (`sibling_tool`, `launch`).

### On the page side (`@dotany/web`)

- **`createShell(cfg)`** (`shell.js`) draws the window around a tool's work and returns `shell`. The parts, made in this
  order, each explained at the top of its file in `shell/`: `window.js` (the top bar with the file's name and state dot,
  Settings, Save As and the window's buttons), `state.js` (the file's state and its dot), `side.js` (side panels),
  `save-as.js` (the save card), `notices.js` (the notice row), `leaving.js` (the close prompt), `opening.js` (opening, the
  empty window with recent files, and the window's keys), `file-menu.js` (the file menu and the menu a tool opens from
  its own buttons), `drop.js` (the drop sheet), `settings.js` (settings: Appearance, Interface, Sound, Language,
  About and the licences), `language.js` (the words on screen), `begin.js` (the shell a tool gets back, and start-up), with `morph.js` (the
  family's motion for a box whose contents change) and `later.js` (how a part calls a part made after it).
- **Parts for the work area** (`parts.js`): the dock's `barBtn` and `pill`, `slider`, `toggleRow`, `chips`,
  `languageRow` and `fitChips`; `new-panel.js` is the New panel that first asks which kind of file.
- **Modules every tool uses**: `backend.js` (the family's commands as functions, and `fileSrc`), `shared.js` (`el`,
  `icon`, `store`, `sound`, `reduce`, `EASE`), `keys.js` (the shortcut key and the way each system writes keys),
  `paths.js`, `fonts.js`, `sound.js` (the one soft click, synthesised), `inkfall-input.js` and `core.js` (the caret that
  glides and letters that settle in), `gestures.js` (the Mac's trackpad), `window-frame.js` (the one layer between the
  page and the window around it), `i18n.js` (words, below) and `licences.js`.
- **`family.css`** comes first in the built stylesheet; a tool's own `src/styles.css` only adds to it. Its `:root` block
  holds the colours, the glass, the type and the motion (`--t-1`, `--t-2`, `--t-3`, `--e-out`); a tool uses those
  variables and writes no values of its own.

## How a program uses it

**Rust.** A program's `Cargo.toml` has `dotany-base = { path = "../../shared/rust" }`. It describes itself once:

```rust
pub static TOOL: Tool = Tool {
    name: "DotAny Starter",          // the window's title and the about page's name
    folder: "Starter",               // %APPDATA%\DotAny\Starter
    data_env: "DOTANY_STARTER_DATA", // moves that folder (tests use it)
    user_dir: "Documents",           // where a first open or save starts
    opens_files: true,
    extensions: &["txt", "md"],      // what the open dialog shows and what can be opened at all
    sniff: any_bytes,                // a look at the bytes before anything unsaved is asked about
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};
```

and starts like this (`starter/src-tauri/src/app.rs` is the whole of it):

```rust
dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
    .manage(Docs::default())
    .invoke_handler(dotany_base::handler![open_file, estimate_save, save_file])
    .run_family(tauri::generate_context!())
```

`open_file` reads the file, keeps what it needs in a `PerWindow<T>` (a value per window, made on first use by
`of(label)`), and calls `dotany_base::opened_in(app, label, &path)`. `save_file` writes with `atomic_write` and calls
`saved_in(app, label, &target)`. The tool's `forget(app, label)` drops what it kept for a window when its file closes or
the window goes. A program that opens its own windows (Text, and the hub) uses `builder_with`, makes windows with
`open_window_as` or `open_document_window`, and ends with `run_family_with` or `run_family_then` (the latter runs a
function as the program exits).

**Page.** The build is `shared/scripts/build.mjs`: a program's `scripts/build.mjs` calls `buildTool({ root, name, icons })`,
which writes `src/icons.js` from `lucide-static`, writes `src/notices.generated.js` (the licence notices, from what is
really compiled in), bundles `src/main.js` into `dist/` with `@dotany/web` aliased to `shared/web`, and joins
`family.css` and the tool's `src/styles.css`. The program's `main.js` is:

```js
import { createShell } from '@dotany/web/shell.js';
const shell = createShell({
  name: 'DotAny Starter', icons: LUCIDE, words: WORDS, notices: () => import('./notices.generated.js'),
  formats, formatOf, open, show, clear, dirty, discard, estimate, save, saved,   // the tool's own
});
shell.begin();
```

The tool draws its work in `shell.stage`, calls `shell.changed()` when the file changes, and reads and writes through
its own `backend.js`, which calls its own commands with `invoke`. The page finds what the program was started with in
`window.__DOTANY__` (`start`: a file to open, `new`: a kind of file to make, and whatever the tool's `extra` added).

## What `createShell` reads

Everything but `name`, `icons`, `notices` and `words` is optional. Each switch is explained at the top of the part of
`web/shell/` that reads it; `createShell` in `web/shell.js` says which part that is.

| Switch | What it does |
|---|---|
| `open(path)`, `show(r)`, `clear()`, `cantOpen` | The tool's open command and what to draw or undraw. `canOpen(path)` is the tool's own look before anything unsaved is asked about. |
| `formats`, `formatOf(doc)`, `save(path, fmt)`, `saved(r, fmt)`, `estimate(fmt)` | The save card: its formats, the file's own format, the write, and the new file's size. `estimate` may answer `{ size, note }` to say a little more after the size. |
| `dirty()`, `discard()`, `beforeGuard()`, `beforeSaveAs()` | Unsaved work: whether there is any, forgetting it, and the tool's last word (settling a crop, text being typed) before it is asked about or the card opens. |
| `saveInPlace: { save(), auto() }`, `shell.saveReady(on)` | The file is saved where it is: Save `{Mod+S}` in the file menu and beside the state, Save As on `{Mod+Shift+S}`, and the close prompt's Save saves in place. `auto()` is asked when the state dot would turn dark, for auto-save. |
| `newFile: { label, run }`, `newFile.groups`, `newFile.create(item)`, `multiWindow: true` | New: a second button in the empty window, a menu item and `{Mod+N}`. With `groups` it first asks which kind of file (`new-panel.js`). With `multiWindow` a new file opens in another window while a file is open, so nothing is asked. |
| `drop(paths)`, `dropLabel()`, `dropTitle`, `fileKind`, `openLabel` | What dropped files do, what the drop sheet and the empty window say, and the open dialog's filter name. |
| `alwaysWork: true` | The work area shows with nothing open; no empty page. |
| `print()` | Print… under Save As… and `{Mod+P}` while a file is open; the base's `print_window` opens the system's print window. |
| `count: true`, `shell.setCount(text)` | A quiet number before Settings, hidden with the top bar's other own parts while the window is empty. |
| `oneFormat`, `formatsFor(doc)`, an extension like `.tar.gz` | The card has no format row for one format, offers only the formats this file can become, or takes a two-part extension off the name. |
| `export: true` on a format, or `exported: true` from `save`, or `status: 'copied'` | A copy, not the file: the window stays on what is open and says Saved as and the copy's name. The tool's Rust does not call `saved` for it, so it stays off the recent list. |
| `liveNote`, `noteAtFoot`, `saveRows`, `saveNote`, `saveLine`, `money(bytes)`, `cardScrolls`, `dimWhileSaving` | The card's quiet line, its options, its size wording, scrolling when it is taller than the window, and dimming Save while a large file is written. |
| `askFirst: true`, `shell.retrySaveAs()`, `shell.askEncoding(title, retry)` | The tool may ask something before writing (Save with UTF-8) while the card stays, then writes again. |
| `shell.notice(text, buttons, { icon, id })`, `noticeKinds`, `shell.showNotice(kind)`, `noticeMoved(on)`, `keptMine()`, `failWords` | Notices of the tool's own in the family's row under the top bar; the work is told when the row comes or goes. `keptMine` is where a tool that keeps its own unsaved state marks the file unsaved after Keep Current Edits. |
| `shell.openPop(button, items, pick, { up, at })`, `shell.closePop()`, `fileItems(doc)` | The family's menu for the tool's own buttons (under the button, upward from a bar, or at the pointer), and the tool's own items in the file menu after Open…. |
| `shell.settingRow(page, { id, title, note, get, set })`, `shell.settingsPage(key, title, before)`, `soundNote`, `licence`, `licences`, `notices` | A switch or a page of the tool's own in settings, the sound row's line, the program's own licence when it isn't MIT (the first row of the licence list; Books is GPL-3.0-or-later), and the tool's own rows in the licence list (`[name, role, licence, 'font' \| 'core']`). |
| `sideMorph: true` | From one side panel to the next the glass stays: the height eases and the new contents fade in. |
| `focus()`, `editors`, `textMenus`, `keys(e)`, `beforeShow()`, `startup()` | The work gets its focus back when the family's parts close; keys the window blocks reach the tool's editors first; the tool's own shortcuts; and what the window waits for before it shows. |

## The rules a program keeps

**Saving.** Write with `atomic_write` (or `atomic_produce`), then call `saved_in`. Never write over the open file in
place. A save that is a copy (an export) does not call `saved_in`, so the window keeps its file and the copy stays off the
recent list. A write that fails answers `{ status: 'failed', kind, detail }`; the page words it and the window keeps
everything. If the file changed on disk, the page shows a notice; Keep Current Edits (`keep_mine`) keeps what is on
screen and marks it unsaved, and a save checks the file has not changed since.

**Settings.** The family's settings are `dotany-theme`, `dotany-labels`, `dotany-sound`, `dotany-lang` and `dotany-autosave`
(`family::SHARED_KEYS`). `set_setting` writes them to the shared file as well as the tool's own, and a change made in any
program reaches every open window (`shared-settings`, handled in `web/shell/settings.js`). A tool's own settings (a
microphone, a volume, a list of recent formats) stay in its own `settings.json` and are written with the same command.
There is no Check for Updates in a tool's settings: the about page says the version and has the licences.

**Recent files.** One list, `recents.json` in `%APPDATA%\DotAny\Shared` (`DOTANY_FAMILY_DATA` moves it; in a test whose
tool data folder was moved, a `family` folder inside it is used instead, so a test never reaches the real one). `opened`
and `saved` add to it with the tool's folder name and the time; a tool's empty window and file menu show that tool's own
files from it, at most five, and the file that is open is not among the menu's; the hub shows all of them. The tool's
`settings.json` keeps its own copy of eight. A tool's older list joins the shared one once (`imported`).

**The window.** The top bar is the only bar and holds the file's name, Settings, Save As and the window's buttons; nothing
else draws a frame. WebView2's own right-click menu is blocked (text fields keep theirs, for cut, copy and paste), and so
are F5, Ctrl+R, Ctrl+F, Ctrl+P and the other browser keys, so a page never reloads over unsaved work and the browser's
find bar and print preview never show; the keys still reach the tool's own `keys`. The window is made in the colours
the settings chose, so nothing flashes, and `window_ready` gives it focus once its page is up; `set_frame` hands the
window's colours back to the system, so System chosen after a fixed theme follows the system again. A window can
close only after its page has settled unsaved work (`close_window`), which is why `close-requested` goes to the page
first. A tool uses the family's modules rather than its own: no colour, motion or
type size outside `family.css`'s variables, no second menu, card or panel of its own where the family has one.

**Panics and locks** (`locks`). Programs are built with `panic = "unwind"`, and what a panic ends depends on where it
happens. A command marked `#[tauri::command(async)]`, an `async fn`, a thread the tool starts and code inside
`catch_unwind` are caught: that command never answers and the program goes on. A plain `#[tauri::command]` runs on the
main thread inside WebView2's own call, which a panic cannot pass: the program ends at once, and `crash.log` has a second
entry, "panic in a function that cannot unwind". A panic in a window event or in `setup` ends the program too. A
`std::sync::Mutex` held by a command that panics stays poisoned, so every later `.lock().unwrap()` panics as well. A lock
held while code runs that can trip over a file's bytes (a reader, a decoder, a writer) is taken with `dotany_base::lock`
(`read_lock` and `write_lock` for an `RwLock`) at every place it is taken, and the code under it changes the data only by
plain assignments after the work is done, so a panic leaves the data as it was. A lock held only for an assignment, a
clone or a list change keeps `.lock().unwrap()`. Better still, read and decode first and take the lock only to put the
result in.

**One window per file on the Mac.** On the Mac a tool is one program with one Dock icon and each file is a window of it.
On Windows each file is a program of its own and the same code runs with one row. A tool that keeps a file per window
builds with `builder_per_window`, keeps its own state per window (`PerWindow`), and uses the calls that name the window.
The calls without a window (`opened`, `saved`, `current`, `close_file`, …) mean the window `main`; they are for a tool
built with `builder`, which keeps one file for the program.

| | |
|---|---|
| `builder_per_window(&TOOL, FileWindows { forget, extra })` | The base opens every window itself: the first is `main`, later ones `main-2`, `main-3`… `extra` is what every page finds in `window.__DOTANY__` besides `start` and `new` (`no_extra` for none). |
| `forget(app, label)` | Drops the tool's own state for that window: called when its page closes the file (`close_document`) and when the window goes. `Tool::on_close` is not called for such a tool. |
| `PerWindow<T>` | The tool's own state, a value per window: `of(label)` (made if missing), `get(label)`, `remove(label)`. Put the locks inside `T`. |
| `opened_in`, `saved_in` | Instead of `opened` and `saved`. |
| `base.current_in(label)`, `start_folder_in(label)`, `unchanged_since_read_in(label)` | Instead of `current()`, `start_folder()`, `unchanged_since_read()`. |
| `close_file_in(app, label)`, `untitled_in(app, label)` | For Rust that puts a file down itself, and for a window now holding work with no file (a picture pasted in). |
| `set_same_bytes_check(fn(&AppHandle, &str, &Path) -> bool)` | Lets a tool that still holds the bytes it read say that a file whose time changed has the same bytes, so nothing is announced. |
| `open_document_window(app, spec)`, `hand_over(app, label, path)` | For a tool that opens its own windows: a window made while others are open is cascaded on the Mac; a file goes to an empty window as the base would hand it. |

The base keeps a table of open files, a row per window label, made with the window and dropped with it. A row holds
where the window's file is, what it was when last read or written (for the "changed by another program" notice), its
watcher, and, on the Mac, whether the page can take a file yet and whether one is on its way.

## What the whole family shares

`rust/src/family.rs` keeps one folder, `%APPDATA%\DotAny\Shared`, changed under a lock by every program so that two
programs saving at once never lose each other's line:

- `recents.json`: the shared list of recent files, up to 96 entries.
- `settings.json`: the shared settings above. Each program brings its own choice of a key nobody has brought in or set
  yet as it starts (`dotany-shared-seeded` lists the keys that have been), so a setting that becomes shared loses nobody's
  choice and an older choice never overwrites a newer one.
- `--new` and `--new=csv`: the hub's New starts a tool this way; a tool with `newFile` runs it as it opens, with the kind.

## Languages

Chinese is the source. The Chinese written in the code is the key, and it is what Chinese shows. Every other language is
a table of its own, Chinese to that language, one JSON file per language: the family's words in `web/lang/<code>.json`,
a program's own in its `src/lang/<code>.json`. Nine languages ship: 简体中文, 繁體中文, English, 日本語, 한국어, Español
(Latin American), Português (Brazilian), Français and Deutsch. A program hands its tables to
`createShell({ words: { en, ja: () => import('./lang/ja.json'), … } })`. `web/i18n.js` explains how a key is written:

- `T('另存为 · {Mod+S}')`: a shortcut, as the keyboard says it.
- `T('删除全部 {n} 段录音？', { n })`: a value the code fills in. The translation may say a count its own way,
  `{n, plural, one {# recording} other {# recordings}}`, with `Intl.PluralRules` choosing.
- `T('关闭::混响')`: the same Chinese meaning something else (Off, not the window's Close). Chinese shows what is before
  `::`; the rest tells the translator where it is.
- `num(n)`, `size(bytes)`, `date(d, options)`: numbers, file sizes and dates the language's way, through `Intl`.

A word a table has not got falls back to English, then to the Chinese. System takes the first of the system's languages
(`navigator.languages`) there is a table for, and English when there is none. A language other than Chinese and English
is loaded the first time it is chosen, before the window shows, so a language nobody picks costs nothing.

**Adding a language:** a line in `LANGS` in `web/i18n.js` (its code, its own name, its `lang` and `Intl` locale),
`web/lang/<code>.json`, and `src/lang/<code>.json` beside each program's `en.json`, each given to `words` as a loader.
Settings › Language lists `LANGS` by itself.

## The Mac

Everything below is behind `cfg(target_os = "macos")` in Rust and `platform === 'mac'` on the page; Windows is
unaffected. Programs are Apple silicon only, macOS 13 or later.

- **Frame.** `platform/macos.rs` gives what `platform/windows.rs` gives. The window keeps the system's own frame (overlay
  title bar, hidden title) with the red, yellow and green buttons on the top bar's left, placed by the base; the page
  leaves room for them (`web/window-frame.js`) and draws none of the family's window buttons. In a window the top bar never
  steps aside; in full screen the buttons are the system's and the bar gives up their room. A click that brings a window
  forward acts only on the top bar; everything else in the window just comes forward.
- **The menu bar** has the program's menu (About, Settings… ⌘,, Services, Hide ⌘H, Hide Others ⌥⌘H, Show All, Quit ⌘Q),
  a File menu for a program that opens files (`Tool::opens_files`: New ⌘N when the page says its tool makes new files,
  Open… ⌘O, Open Recent, Close ⌘W), Edit (Undo ⌘Z, Redo ⇧⌘Z, Cut, Copy, Paste, Select All) and Window (Minimize ⌘M,
  Zoom, Enter Full Screen ⌃⌘F, Bring All to Front, the open windows, and Close ⌘W when there is no File menu). Its
  words come from the page's language tables (`set_menu_words`); Rust never translates. With no window open,
  Settings…, About and New open a window and tell its page once it is ready.
- **Opening.** Files opened in Finder, dropped on the Dock icon or chosen in Open With arrive as `RunEvent::Opened`, never
  on the command line. A file already open brings its window forward; a window with nothing open takes it; a window
  holding work with no file is never taken over; otherwise the tool opens a new window, cascaded from the one in front.
  Files that arrive before the first window exists wait for it. Open… is the system's open panel as a sheet on the
  window in front. `dotany-<tool>://new?format=…` starts a new document in an empty or new window; the hub opens that
  address.
- **Windows and quitting.** Closing the last window leaves the program running until ⌘Q; a Dock click opens an empty
  window. ⌘Q, the Dock icon's Quit and a logout close each window as its red button does, windows with unsaved work
  first; Cancel in any window's question stops the quit with every window as it was. A new window opens at the size a
  window of the tool was last left at, within the screen's room. If a window's page process ends, the window comes back with its file.
  `set_edited` puts unsaved work in the red button's dot, and `representedURL` follows the page's file.
- **Keys.** `modKey(e)` is ⌘ alone on the Mac (Ctrl or ⌘ on Windows); labels follow the Mac's menus (⌃ ⌥ ⇧ ⌘, ↩ ⎋ ⌫ ⌦
  ⇥); `{Mod+Y}` shows as ⇧⌘Z and `{Shift+F5}` as ⇧⌘↩; `os({ windows, mac })` picks words that name the system's own
  things. A page-wide pinch or swipe back is turned off. `web/gestures.js` reads the trackpad's scroll, pinch, rotate and
  smart zoom.
- **Files.** Only `/` and `:` are refused in a file name on the Mac; names are compared after Unicode normalisation.
  Recent files go to the system's list as well (Open Recent); Recorder, which Finder gives no kind of file, shows the
  family's list there. Fonts are read from `DotAny.app/Contents/Resources/fonts` beside a tool's `.app`.
- **Printing** (`mac_print.rs`) prints the page through WKWebView and the system's print panel as a sheet, with the
  page's backgrounds and `@page` margins as WebView2 prints them. Finder's Quick Actions are `quickaction.rs`.
- **Packaging.** `scripts/mac-plist.mjs` writes each program's `src-tauri/Info.plist` (its kinds of file, its web address,
  the folder and microphone reasons, the nine languages) from `installer/payload.json`; run it after the formats change.
  `scripts/mac-release.mjs` makes the `.app` files, the disk image and the update package; `scripts/mac-app.mjs` wraps a
  build in the smallest bundle a Mac treats as a program.
- **Test driver** (`testdrive.rs`): in a test (`DOTANY_TEST`, `DOTANY_TEST_PORT` and `DOTANY_TEST_TOKEN`) the program
  answers the few DevTools messages a test's driver sends, since WKWebView has no debugging port; only requests that carry
  the token and no `Origin` header (`testgate.rs`, see "Environment variables").

## What is deliberate

- Every program reads one copy of the fonts and none carries them. An installed program reads `fonts\` beside it (the
  installer puts one copy there and makes HarmonyOS Sans a Windows font, so pages take it from Windows); a program built
  from a checkout reads `shared/fonts`; `DOTANY_FONTS_DIR` points tests anywhere. With none, the page uses the system's
  fonts. The Rust side keeps no font in memory.
- Settings, recent files and the shared folder are plain JSON next to the user's data, changed under a lock, written
  atomically, and never silently overwritten when broken.
- The page never reloads and never shows the browser's own menus or find bar (see "The window").
- At most five recent files show in the empty window and the file menu; with none, the empty window keeps no room for
  them. The status beside the dot goes quiet after 2.6 seconds, unless it says Unsaved or that it is reading or
  saving. Save As stays pressed while its card is open.
- In English the licence list puts HarmonyOS after Stack Sans, and a tool's own rows go with the fonts or before or after
  Tauri as `licences` says.
- Only `http` and `https` addresses are ever handed to the system's browser, and lengths read from a file are never
  trusted (`limits`).

## Environment variables

The programs read the variables below. They exist for automated testing and for development: a person using DotAny sets
none of them, and a program started without them behaves as it ships. Only whoever starts a program can give it
variables, so none of them lets another user of the computer, or a web page, do what they could not do already. The
two that open a way into a running program for a test's driver, `DOTANY_DEBUG_PORT` and `DOTANY_TEST_PORT`, ask for
more than being set, as their lines say.
WebView2 reads variables of its own as well (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` and others); those are Microsoft's.

**A test driving the program**

- `DOTANY_TEST`: the program is under an automated test. Its page lets the test read and drive it (`shell.test`) and
  offers made-up languages that show where a layout doesn't adapt; the hub checks for no update and leaves the system's
  settings alone; the variables below marked "in a test" are read only with it.
- `DOTANY_DEBUG_PORT` (Windows, in a test): adds `--remote-debugging-port=<port>` to the family's WebView2, so the test
  reaches the pages through Chromium's DevTools protocol. No `--remote-allow-origins` is added, so Chromium refuses every
  DevTools connection that carries an `Origin` header, as every web page's does.
- `DOTANY_TEST_PORT` and `DOTANY_TEST_TOKEN` (the Mac, in a test): the program listens on `127.0.0.1:<port>` and answers
  the DevTools messages a test's driver sends (`testdrive.rs`). It starts only with a token of 32 to 256 characters of
  base64url, answers only requests whose path begins with `/<token>/`, and refuses any request that carries an `Origin`
  header or names a host other than `127.0.0.1` or `localhost` with its port (`testgate.rs`).
- `DOTANY_TEST_PICK_OPEN`, `DOTANY_TEST_PICK_SAVE` (the Mac, in a test): files holding what the open and save panels
  answer, so no panel is shown.

**Where things are kept**

- `DOTANY_<TOOL>_DATA` (`DOTANY_TEXT_DATA`, `DOTANY_HUB_DATA`, … as each program's `data_env` names it): the program's
  settings folder; its WebView data then goes there too.
- `DOTANY_FAMILY_DATA`: the family's shared folder (recent files, shared settings). `DOTANY_WEBVIEW_DATA`: the WebView data
  every program shares. `DOTANY_FONTS_DIR`: where the family's fonts are read from.
- `DOTANY_HUB_TOOLS_DIR`: where the hub finds the tools it lists and starts.
- `DOTANY_HUB_RUN_KEY`, `DOTANY_HUB_SHELL_ROOT`, `DOTANY_HUB_ASSOC_ROOT`, `DOTANY_ARCHIVE_SHELL_ROOT` (Windows): keys under
  `HKEY_CURRENT_USER` used in place of the real ones for starting with Windows, the right-click menu and file types. In a
  test without them the hub writes to keys of its own.
- `DOTANY_TEST_SERVICES_DIR` (the Mac): where Quick Actions are written instead of `~/Library/Services`.

**What a program does differently**

- `DOTANY_TEST_LINKS`: a web address a file links to is written to this file instead of opened.
- `DOTANY_MEASURE`: the program writes its start-up timings to this file and quits. `DOTANY_REST_AFTER_MS`: how long a
  window behind others waits before its page lets go of memory.
- `DOTANY_TEST_PANIC` (in a test): a thread of the program panics on purpose, to show the crash log is written.
- `DOTANY_TEST_NO_MEDIA_FOUNDATION` (Audio, Slides and Video on Windows): as if Media Foundation were missing.
- Audio: `DOTANY_TEST_TAP` (a file that gets a copy of all the sound sent to the speakers), `DOTANY_TEST_FAIL_WRITE` (an
  export fails partway). Image: `DOTANY_NOGL` (draws without the GPU).
- Recorder: `DOTANY_RECORDER_TEST_TONE` (a known tone in place of the microphone), `DOTANY_RECORDER_TEST_STALL_MS` and
  `DOTANY_RECORDER_TEST_FAIL_AFTER_MS` (the disk stalls once, or stops taking writes, after that long); on the Mac, in a
  test, `DOTANY_RECORDER_TEST_DONT_ASK` (the system's microphone question is not asked) and
  `DOTANY_RECORDER_TEST_NEVER_HEAR` (the microphone is taken as refused, whatever the system says).
- Video: `DOTANY_VIDEO_TEST_STILL` (in a test: a film doesn't start playing as it opens), `DOTANY_VIDEO_FAIL_WRITE` (a save
  fails after that many bytes). Slides on the Mac, in a test: `DOTANY_TEST_SCREENS=two` (a second screen is made up).
- Hub: `DOTANY_UPDATE_URL` (updates come from servers on this computer only, `127.0.0.1` or `localhost`, comma-separated;
  an update must still be signed), `DOTANY_UPDATE_TEST_KEY` (only with it: one more public key an update may be signed
  with), `DOTANY_UPDATE_LAUNCHED` (a file that gets what the update would start, instead of starting it),
  `DOTANY_UPDATE_FIRST_MS` (the wait before the first daily check), `DOTANY_FEEDBACK_URL` (feedback goes to a receiver
  on this computer); on the Mac, in a test, `DOTANY_TEST_FIRST_START` (the login item is set as on a first start) and
  `DOTANY_TEST_AT_LOGIN` (as if started at login).
- Installer: `DOTANY_SETUP_ROOT`, `DOTANY_SETUP_START_MENU` and `DOTANY_SETUP_APPDATA` (where it installs, its Start menu
  folder, the settings folder), `DOTANY_SETUP_HIVE` (where under `HKEY_CURRENT_USER` its records go; programs it starts
  are given keys and folders of the same test), `DOTANY_SETUP_LANG`, `DOTANY_SETUP_THEME`, `DOTANY_SETUP_TEST_DELAY_MS`
  (a wait after each file it unpacks, so an install can be cancelled midway), `DOTANY_SETUP_NO_WEBVIEW2` (as if WebView2 were missing).

**Building and developing**

- `DOTANY_BUILD`: the folder the installer's packer takes the built programs from. `DOTANY_NO_FONTS=1`: Text's and
  Books' builds leave their fonts out of the page.
- `DOTANY_SCAN_OWN`: words the publishing scan in `.github/scripts` must not find.
- `DOTANY_PDF_PLAN`, `DOTANY_FEEDBACK_HTTPS`: inputs of two unit tests that run only when asked for by name.

## Building

A program is built from its own folder (`npm ci`, then `npx tauri build --no-bundle`; see its README). The base has no
`package.json` of its own: each tool installs its own `esbuild` and `lucide-static`, and builds `@dotany/web` from this
folder. The Rust base needs only Cargo. The unit tests inside the source files are part of this repository and run with
`cargo test` (in `rust/`, for the base). The end-to-end tests that drive the real programs, and their sample files, are
not part of this repository.

## Licences

The base is under the repository's MIT licence (see `LICENSE`). `fonts/` carries the licences of
the fonts beside them; the licence list each program shows is generated by `scripts/notices.mjs` from what is really
compiled in. The page starts with the program's own licence and where its source is (`scripts/own-licence.mjs`: the
MIT text of the repository's `LICENSE`, or the program's own `LICENSE`, which only Books has), and gives the address of
the source of every library under the Mozilla Public License.
