# DotAny

DotAny is the home of the family: one window that lists the files every DotAny tool opened lately, starts the tool for
a file or a new document, converts files into another format, installs and updates the tools, and sets which program
opens which kind of file. It never opens a file itself: a double-click on a file still goes straight to its tool. It is a
[Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with esbuild) and a Rust program in
`src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon. Its program is
`dotany.exe` on Windows.

## Which tool opens what

The hub knows these tools by a key. A file the person picks or drops goes to the first tool below that lists its kind
(then to the first that lists it among its rarer kinds); a row of the recent list goes to the tool that opened that
file last. `FAMILY` in `src-tauri/src/app.rs` is the source of this table; each tool also opens a few rarer kinds,
which the open dialog offers but the Default apps page has no chip for.

| Tool | Key | Kinds on the Default apps page |
|---|---|---|
| Text | `text` | docx, doc, rtf, odt, md, txt, json, srt |
| Image | `image` | png, jpg, jpeg, webp, gif, svg, bmp, tiff, ico |
| Video | `video` | mp4, mkv, mov, webm, avi, wmv |
| Audio | `audio` | mp3, flac, wav, aiff, ogg, m4a |
| PDF | `pdf` | pdf |
| Archive | `archive` | zip, 7z, rar, tar, gz |
| Sheets | `sheets` | xlsx, csv, tsv, xls, ods |
| Slides | `slides` | pptx, ppsx, potx |
| Books | `books` | epub, fb2, cbz, djvu, mobi |
| 3D | `model` | glb, gltf, obj, stl, fbx, 3mf |
| Recorder | `recorder` | none (it records, it opens nothing) |

## What you can do

- **Home.** Drop files on it to open them in their tools. Make a new file in the tool that makes it (a document, a workbook,
  a deck, an archive, a recording, a source file, a subtitle file) from a list of kinds that can be searched. Pick up where you left off from the recent
  list, which every tool writes to and the hub only reads; a row says which tool opened the file last, and offers Convert
  when the converter takes it. A file can be forgotten from the list, or the whole list cleared.
- **Convert.** Drop files into the converter, or choose Convert with DotAny in the file manager's right-click menu, and
  pick a format. The converter is the library `dotany-convert` (`../shared/convert`, whose README lists what converts
  to what). The new files are written beside the originals under names that are free, and nothing is overwritten.
  Several files chosen in one right-click arrive as one start each, and those that come within a moment of the first
  join its window.
- **Tools.** See which tools are installed and at which version, whether the latest release has newer ones, and, on
  Windows, install a tool that is not installed yet. Check for Updates looks for a newer release; Update installs it.
- **Default apps.** See which program opens each kind of file. On Windows the person changes it in Windows' own Default
  apps page, which the hub opens at the tool's own entry. On the Mac the hub makes the choice itself and gives the kind
  back to the program that had it when it is switched off.
- **Settings.** Open DotAny when the system starts (Windows: when Windows starts; the Mac: as a login item), the
  right-click menu (Windows) or Quick Action (the Mac), Auto-download updates (whether a newer release downloads by
  itself), appearance, sound and language, all of which the other tools share.
- **Contact.** The address to write to, with a button that copies it, and a box that sends a message to DotAny
  (see "What it sends over the network").

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany.exe` in `release/`
under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the family's,
`shared/scripts/build.mjs`; it also writes `src/icons.js` from `lucide-static`. `src/notices.generated.js`, the licence
notices the About page shows, is written by `shared/scripts/notices.mjs`, with the licences of the converter's C
libraries added by `build.mjs`. The converter writes MP3, so the build of `dotany-sound-encode` puts `libmp3lame.dll`
beside `dotany.exe`; without it MP3 conversion stops with a message. `cargo test` in `src-tauri/` runs the unit tests,
which check the signatures, the sums, the order the release places are asked in and the feedback sender against small
servers on `127.0.0.1`.

The tools are built the same way from their own folders. The release's installer and its packer are in `../installer`.
The Mac's application, its `Info.plist` and its `lproj/` folders are made by `shared/scripts/mac-release.mjs` and
`shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: the family table (`FAMILY`), the windows (the main window and the converter's small one), the commands the
  page calls, the settings that belong to the system, and the update state that the page follows.
- `update.rs`: checking for a release and downloading it: where it is, what is believed, what is verified. `net.rs`:
  fetching and posting over https with the system's own HTTP stack (WinHTTP on Windows, `NSURLSession` on the Mac).
- `feedback.rs`: the Contact page's sender and the draft it keeps. `convert.rs`: turns the page's choices into the
  converter's options.
- `system/`: what DotAny asks of the operating system, with the same functions on each: `windows.rs` (the registry, the
  mutex that keeps one DotAny, the version in an exe) and `macos.rs` (the login item, the Quick Action, Launch Services,
  the version in an `Info.plist`, the programs now running).

**The page** (`src/`):

- `main.js`: the window, the places, the converter box, the tools and default apps pages, settings and Contact.
  `backend.js`: the calls to the Rust side. `lang/`: DotAny's own words in each language, added to the family's.
- `styles.css`: DotAny's own sheet, after the family's (`shared/web/family.css`).

**Shared with the rest of DotAny:** `dotany-base` (`../shared/rust`: the window, settings, the shared recent list,
atomic writes and the platform layer), `dotany-convert` (`../shared/convert`) and `shared/web` (the window frame, menus,
settings pages and licences).

## How it works

**Finding and starting the tools.** The tools are programs beside the hub: on Windows `dotany-<key>.exe` in the folder
`dotany.exe` is in (the installer puts them there); on the Mac, a tool's `.app` in the folder DotAny's `.app` is in.
A tool is installed when its program is there, and its version is the one written into its exe (the version resource) or
its `Info.plist`. `DOTANY_HUB_TOOLS_DIR` points the hub at another folder of plain programs. To open a file, the hub
starts the tool's program with the file's path (on the Mac through the system's `open`, so a tool that is already open
takes the file); to make a new document it starts the tool with `--new=<extension>` (on the Mac, through the tool's own
address, `dotany-sheets://new?format=xlsx`). The tool puts the file on the shared recent list itself.

**One DotAny at a time.** On Windows the first DotAny holds a mutex named for its data folder; a later start leaves its
arguments (`--convert` and the files) in a small inbox folder and wakes the first, which acts on them and comes to the
front. `--autostart` gets the window ready without showing it. On the Mac the system keeps one DotAny.app open and
hands it the files itself.

**The recent list** is `recents.json` in the family's folder, which every tool writes with the tool's name; the hub
reads it, and the page moves only the rows that changed when a tool adds to it.

**Updating.**

- *Where from.* A release is published in two places with the same files by the same names: the site
  (`https://dotany.app/releases/latest/`) and GitHub (`https://github.com/Waldsatte/DotAny/releases/latest/download/`).
  They are asked in that order, because GitHub is often slow or out of reach in mainland China. Which place a file came
  from changes nothing about trusting it. A place that does not answer in time, has no such file, answers something that
  is not a release or has a file that fails a check passes the turn to the next; a place with another after it is
  waited for 5 seconds to connect and 10 without a byte (15 and 30 for the last place and for the installer itself).
- *What is fetched.* To check, three small files of a place: `SHA256SUMS.txt`, its signature `SHA256SUMS.txt.sig`
  (minisign's format, Ed25519) and `manifest.json`, which lists every part of the release with its version. They are
  plain downloads, not GitHub's API. To install, the whole `DotAny-Setup.exe` (on the Mac, `DotAny-mac.zip`, the DotAny
  folder of `.app`s), into the `DotAny-update` folder of the system's temporary folder, named `.part` until it has
  passed.
- *What is verified, in this order.* First, the signature on `SHA256SUMS.txt`, by one of the public keys in
  `TRUSTED_KEYS` (the secret half is kept apart from the repository and its build; each release's list is signed
  by `.github/scripts/sign-update.mjs`). Second, `manifest.json`'s SHA-256 against that list, so the versions in it are
  covered by the signature. Third, that no part of the release is at a lower version than what is installed. Fourth,
  the installer's SHA-256 against the list. If any of the four does not come or does not pass, the next place is tried
  from the start. A check believes a version only after the first two, so a place that has been tampered with can
  neither offer a version nor claim there is none.
- *What a check decides.* The release's version of each installed part is compared with the installed one (`hub` for
  DotAny itself, a tool by its key). A place with something newer is the answer at once. A place with nothing newer is
  kept, but the next place is still asked (briefly), so a site that lags, or says it is up to date, cannot hide a newer
  signed release on GitHub. A place with a lower version than what is installed is passed over. An update needs a release
  with something newer than what is installed; installing a tool that is not installed yet needs only a release that
  carries it.
- *When.* A check runs when Check for Updates is pressed and, on its own, once a day while DotAny runs (the first about
  twenty seconds after it starts, if a day has passed since the last answer). The last answer is kept in the data folder
  (`update-manifest.json`) and read again against what is installed, so a start within the day knows what was found.
  With Auto-download updates on (the default), a check that found something newer downloads and verifies it by itself
  and the page says Update ready to install; nothing is installed until Update is pressed. With it off, a check only
  checks, and Update downloads and installs in one go.
- *Starting it.* The downloaded installer is hashed again right before it starts, since it may have waited for days in a
  folder that anything running as the user can write to; one that changed is deleted, and the update says it failed.
  The record of what was verified is kept in memory only, so after a restart nothing counts as downloaded. Windows
  starts the installer as `DotAny-Setup.exe --update --root <folder> [--tools <keys>]`, which asks the person to close
  open DotAny programs, installs, and opens DotAny again. On the Mac the package is unpacked with `ditto` into a folder
  beside DotAny's, each new `.app` must carry a `com.dotany.` bundle identifier, DotAny itself must be in it, and each
  `.app` that is in DotAny's folder now (and DotAny's own) is replaced by the package's; the old ones are moved aside,
  not deleted, until every new one is in place, and any step that fails puts back what was moved. A tool that was
  dragged to the Trash is not brought back.

**What it sends over the network.** Two things, and nothing else.

- *The update check and download.* `GET` requests to the two places above for the files named, one `User-Agent` header
  (`DotAny/<version>`), and no body. Nothing about the person, the computer or what is installed is sent. This happens
  when Check for Updates or Update or Install is pressed, once a day in the background, and when a newer version is
  downloaded by itself (see "When"). The page itself makes no requests; its content security policy allows only the window's own
  commands.
- *The Contact page.* When Send is pressed, one `POST` of `{ "text", "version", "lang", "system" }` as JSON to
  `https://dotany.app/feedback`: the text typed, DotAny's version, the language on screen, and `windows` or `macos`.
  Nothing is sent without that press. It is tried once, with a short wait. The receiver's answer is read as: `200` with
  `{"result":"accepted"}` is sent; `413` too long (the box takes at most 4000 characters), `429` too many from this
  address within a minute, `503` could not keep it just now; any other answer is refused; no answer is offline.
  Until it is sent, the text is kept in the data folder as `feedback/draft.json`, so a message that could not be sent is
  in the box again the next time, and once one is sent the draft is removed.

## What it is careful about

- **Only its own hosts.** Only https is let through, and only to the two places' own hosts (`dotany.app`, `github.com`)
  and GitHub's download hosts (`*.githubusercontent.com`); the Contact message goes to `dotany.app` on port 443 alone.
  Redirects are followed by the hub itself, one at a time and at most five, each address checked before it is asked; the
  Contact post follows none. The size of a download is bounded (512 MiB for the installer, 1 MiB for the small files).
- **Nothing installed on trust.** What is refused: a signature by no trusted key, a signature that is not one, a manifest
  the signed list does not name, a manifest of a format the hub does not know, a release lower than what is installed,
  an installer whose sum is not the listed one, an installer that changed after it was checked, and on the Mac a
  package that is not made of DotAny's own `.app`s. A refused download is deleted, and a check never says that a file
  failed its check (it says it could not read the release), because that sentence is about a downloaded file.
- **Only its own keys.** On Windows only the current user's part of the registry is written, and only keys of its own
  (the `DotAny` value under `Run`, and `DotAnyConvert` under each file kind's `shell`), so turning a switch off takes
  away exactly what turning it on added. Starting with Windows covers DotAny alone, never the tools.
- **The system's proxy and certificates.** WinHTTP and `NSURLSession` use the system's proxy settings (including a PAC
  script) and certificate store, so an office whose proxy inspects TLS with its own root works, and DotAny carries no
  HTTP library of its own.
- **Tests stay on this computer.** `DOTANY_UPDATE_URL` and `DOTANY_FEEDBACK_URL` are taken only when they name a server
  on `127.0.0.1` or `localhost`, and `DOTANY_UPDATE_TEST_KEY` adds a key only together with the first.
- **Closed files.** The converter never opens an original for writing, and a conversion that stops keeps the files it
  finished.

## Windows and the Mac

- **Windows.** The window has no system title bar: the page draws its own buttons. `windows-app-manifest.xml` declares
  Windows 10 and 11 support, which Windows requires before it shows the snap layouts over the maximise button, and
  asks for the version 6 common controls the open dialog uses. Starting with Windows is a value in
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` that starts `dotany.exe --autostart`; the installer sets it first
  (on unless the person or `--no-autostart` said off), the hub's own start never changes it, and after that it is the
  person's choice. The right-click menu is one verb on each kind the converter takes, under
  `HKCU\Software\Classes\SystemFileAssociations`. Which program opens a kind is read from the user's own choice first
  (`UserChoice`), then the class registration, and called DotAny's when the program's name or its command contains "dotany". The page
  sends the person to Windows' Default apps because Windows lets only the person change it.
- **The Mac.** One `DotAny.app` shows its window; opened as a login item it stays out of the Dock until it shows a
  window. Opening at login is `SMAppService`, turned on once by the first start from the .app and never again after that,
  the right-click menu is a Finder Quick Action, Convert with DotAny
  (`dotany_base::quickaction`), and files dropped on the Dock icon go to the converter. Default apps are set with
  `NSWorkspace.setDefaultApplication`: giving a kind to a DotAny tool is answered at once, and giving it back to another
  maker's program waits for the system's own question to the person. The program that had a kind is remembered in
  `default-apps-before.json` in the data folder so it can be given back. No tool is installed on its own on the Mac: all
  of them come in DotAny's folder, an update brings up to date what is there, and a tool taken out of the folder stays
  out. In Chinese, the Contact page's copy button says 拷贝 where Windows says 复制.

## Licence

DotAny is under the MIT licence, as the rest of DotAny is: see `LICENSE` at the top of the repository. The licences of
the libraries carried in the program are on its About page, under Licenses & Credits.
