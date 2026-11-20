# DotAny Setup

DotAny Setup is the installer for the whole DotAny family on Windows: one program, `DotAny-Setup.exe`, that carries
the hub and the eleven tools and installs all of them or the ones you pick. The same program without what it carries
is the uninstaller. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (HTML, CSS and plain JavaScript,
with no build step; Tauri builds the folder into the program) and a Rust program in `src-tauri/`. It runs on Windows
only: on the Mac, DotAny is a folder of applications that is dragged to Applications.

Every file it installs is appended to the installer as one zstd stream. The stream's 64 MB window is larger than any
one program, so the compressor finds what a program has in common with the one before it (the same crates, the same
Tauri runtime) and the download holds that about once.

## What it installs, and where

The window has four steps: Full Installation or Custom, and the folder; for Custom, the tools (the hub, DotAny, always
comes); the installation; and the last step, with Launch DotAny at login and Open DotAny.

DotAny installs for the person signed in, with no administrator prompt, in `%LOCALAPPDATA%\Programs\DotAny`, where
other per-user programs such as VS Code put theirs. Change opens the system's folder picker; a folder that isn't called
DotAny gets a `DotAny` folder inside it. Everything is in one flat folder, because the programs find each other beside
themselves (`sibling_tool` in `shared/rust/src/platform/`):

```
DotAny\
  dotany.exe                 the hub
  dotany-text.exe …          each chosen tool
  libmp3lame.dll             once, for the hub's converter, Audio and Recorder (the same LAME 3.100 build)
  fonts\                     the family fonts and their licences, once
  uninstall.exe              the uninstaller
  install.json               exactly what this install put on the computer
```

The Start menu gets a `DotAny` folder with `DotAny` (the hub) and `DotAny <Tool>` for each tool.

## What it writes to the registry

Everything is under `HKEY_CURRENT_USER`, so no administrator rights are needed to write it or to update it later.
`src-tauri/src/reg.rs` writes every key.

**Each tool, for each type of file it opens** (`formats` in `payload.json`): a ProgID `DotAny.<Tool>.<ext>` with the
tool's icon and open command; a value in `Software\Classes\.<ext>\OpenWithProgids`, so the tool is listed under Open
with; `Software\Classes\Applications\dotany-<tool>.exe` with the types it opens; and its capabilities under
`Software\DotAny\<Tool>`, named in `Software\RegisteredApplications`, so Windows' Default apps settings list it as
`DotAny <Tool>`. Recorder registers no type: it only writes recordings, and WAV opens in Audio.

**Which program opens each type.** An install makes DotAny the default for a type by writing the default value of
`Software\Classes\.<ext>`, by the three lists of `claim` in `payload.json`:

- `never`: html, htm, ts, nc, db, mod and snd are only listed under Open with. html and htm stay the browser's; the
  others are also other programs' files (ts is MPEG-TS video, nc NetCDF data, db any program's database, mod some
  camcorders' video, snd other kinds of sound file).
- `polite`: code and data files (py, js, mjs, tsx, jsx, css, json, jsonc, yaml, yml, toml, ini, xml) become DotAny's
  only while no other program is their default, neither for this person (HKCU) nor for the whole computer (HKLM).
- Every other type becomes DotAny's. A program the person had set in HKCU waits in the same key (value
  `DotAny.Before`) and is put back on uninstall. A default set for the whole computer (HKLM) needs nothing: DotAny's
  only stands over it, and it is the default again once DotAny's goes.

A choice the person made in Windows (the `UserChoice` key Windows writes when someone picks a default app) always wins
and is never touched. An upgrade or an uninstall puts back or removes a default only while it still names DotAny, so a
program that took a type after the install keeps it, through upgrades too. Afterwards the hub's Default Apps page shows
which types open in DotAny and opens Windows' Default apps settings at the tool's own entry, where the person changes
them.

**The fonts**, one value per file (see Fonts below).

**Starting with Windows**: `Software\Microsoft\Windows\CurrentVersion\Run`, value `DotAny` =
`"<folder>\dotany.exe" --autostart`, with which the hub gets its window ready without showing it. It is on by default.
Launch DotAny at login on the last step switches it at once, and `--no-autostart` leaves it off in a quiet install.
The hub has the same switch in its Settings and writes the same value; its own first start leaves the value as the
install set it.

**The rest**: the entry in Settings › Apps (`Software\Microsoft\Windows\CurrentVersion\Uninstall\DotAny`, with the
uninstaller and its quiet form), and `Software\DotAny` → `InstallLocation` and `Version`, which tell the hub and the
next installer where DotAny is.

## Fonts

The family fonts are installed once, in `fonts\`, with their licences, and every program reads that copy
(`shared/rust/src/fonts.rs`): Stack Sans Text and Headline, HarmonyOS Sans SC Regular and Bold, and the fonts of the
languages that need their own (HarmonyOS Sans TC, Noto Sans JP DotAny, Pretendard).

**HarmonyOS Sans SC becomes one of the person's own Windows fonts**, and so do the language fonts. The install writes
one value per file under `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts` (`DotAny HarmonyOS_SansSC_Regular
(TrueType)`, an `.otf` file as `(OpenType)`), naming the file where it is in `fonts\`: no copy, the file unchanged,
its licence beside it. Windows keeps one copy of a font for every program, so a DotAny page takes that copy instead of
loading 17 MB of its own, and the fonts appear in other programs' font menus too. `shared/web/fonts.js` takes Windows'
copy only when it is DotAny's version of the font (in 2.400 「（」 is half a character wide), not an older HarmonyOS
Sans SC the person already had. Windows is told at once (`AddFontResource`, `WM_FONTCHANGE`), so nothing waits for the
next sign-in. An upgrade leaves a font that hasn't changed where it is, and lets go of one that changes first. The
values go with the uninstall; a tool left out by Custom keeps them, because the hub and every tool need them.

Stack Sans stays a font only DotAny's pages load: its two files are small, and a person who has a Stack Sans of their
own would otherwise see two of the same name in their font menus. Books' serif (Noto Serif SC) and Text's code font
(JetBrains Mono) are inside those programs.

## How an install runs, and why cancelling leaves nothing

1. **Stage:** the chosen files are unpacked into `<folder>\.dotany-staging` (hidden) and checked against their
   SHA-256. Nothing else is touched. Cancelling, or a failure, removes it, along with the install folder if this
   install made it.
2. **Commit:** existing files move aside into `.dotany-old` and the new ones move in. Then the registry, the Start
   menu and `install.json` are written, and the old files are dropped. If something fails, the old files go back and
   the earlier install's keys are written again. Tools left out this time (Custom over an earlier install) are removed
   with their keys.

A running tool doesn't stop an upgrade: Windows lets a running program be renamed, though not deleted, so its old file
waits in `.dotany-old` until next time. The hub has no unsaved work: it is asked to close (as its close button does),
and if it hasn't gone within 5 seconds it is ended, then its WebView2 processes, then it is started again after the
upgrade. Ending a program alone and leaving its WebView2 to go by itself would leave some of Windows' committed memory
held until the next restart; ending its WebView2 first would have the program, still running, make its window again
with a new WebView2 browser that outlives it (`system::end`). A program the installer starts gets none of the installer's handles, so a
script waiting on `DotAny-Setup.exe --quiet` isn't held up by DotAny.

The page follows the real work: the line goes as far as the files are unpacked, a row is ticked once its files are in,
and when everything is unpacked the line is full for one frame before the commit. Nothing waits on purpose. A failure
returns to the first step and says why in the bottom-left corner: not enough space, no permission, or the system's own
message.

Every DotAny window, the installer's included, is drawn by Microsoft Edge WebView2. Where it is missing (some older or
trimmed Windows systems), the installer says so in a system message and offers Microsoft's download page; a quiet
install goes ahead and reports it in its log.

## Quietly, for administrators

```
DotAny-Setup.exe --quiet [--root <folder>] [--full | --tools text,audio] [--no-autostart] [--log <file>]
uninstall.exe --quiet [--remove-settings] [--log <file>]
```

Without `--tools` every tool is installed. Exit codes: 0 done, 1 cancelled, 2 not enough space, 3 not allowed to write
there, 4 a DotAny program is open, 5 anything else. `--log` writes the result as JSON.

## The update mode

The hub's Update, and its Install for a tool that isn't installed yet, download the whole `DotAny-Setup.exe`, check it
against the release's signed `SHA256SUMS.txt` (`hub/src-tauri/src/update.rs`), and start it in its update mode:

```
DotAny-Setup.exe --update [--root <folder>] [--tools text,audio] [--quiet] [--log <file>]
```

- It updates the install in `--root` (else the one the registry knows, else the default folder): every tool already
  there stays and comes up to this release, and `--tools` adds tools. Starting with Windows stays as it was.
- There is no choosing step. Open DotAny programs are named first (Update DotAny, Close DotAny Text first.); once they
  are closed it goes on by itself, through the same stage and commit.
- The hub needs no closing: it is ended for the commit and opened again with its window, and the installer's window
  closes; there is no last step.
- With `--quiet`, an open program ends it with code 4 and nothing changes; DotAny is only started again if it was
  running.

## Uninstalling, and what it leaves

`uninstall.exe` is this program without its payload, and Settings › Apps runs it. It copies itself to `%TEMP%` and runs
from there, so it can remove its own folder. Its window has two pages (Uninstall DotAny, then Uninstalled). If a tool
is open, it says which one and stops; nothing is removed until the person closes it.

It removes exactly what `install.json` lists: the files, the keys and the values, the keys the install had to create
once they are empty, the fonts' registration, and the shortcuts; each type's default goes back to the program that
had it. It also removes the value that starts the hub with Windows (also when the hub's Settings turned it on later),
the hub's Convert with DotAny entries in the right-click menu, and Archive's own right-click entries, which the
programs write themselves when those settings are on and which would point at a removed program.

Files the person put in the folder stay, and so does the folder if it isn't empty. Settings and recent files
(`%APPDATA%\DotAny`) stay unless Also remove preferences and history is switched on (`--remove-settings`).

## Building and packing

The installer packs the programs as they are built, so every program is built first. From the top of the checkout, in
Git Bash, as the release workflow does it (`.github/workflows/release.yml`):

```bash
export CARGO_TARGET_DIR=C:/build/target DOTANY_BUILD=C:/build/programs
bash .github/scripts/build-programs.sh windows
cargo build --release --manifest-path installer/src-tauri/Cargo.toml
mkdir -p "$DOTANY_BUILD/cargo-target-installer/release" out
cp "$CARGO_TARGET_DIR/release/dotany-setup.exe" "$DOTANY_BUILD/cargo-target-installer/release/"
"$CARGO_TARGET_DIR/release/dotany-pack.exe" --repo "$PWD" --out "$PWD/out"
```

`build-programs.sh` puts each program in `$DOTANY_BUILD/cargo-target-<name>/release/`, the layout `payload.json`
reads (`{build}` in it). The installer's Cargo package builds two programs: `dotany-setup.exe`, the installer itself,
and `dotany-pack.exe`:

```
dotany-pack [--repo <DotAny folder>] [--setup <dotany-setup.exe>] [--out <folder>] [--level 1-22]
```

It reads `payload.json`, takes the programs from `DOTANY_BUILD` (or, without it, from the folder `dotany-pack.exe`
itself sits in, when that is `<build>/cargo-target-installer/release/`), measures each file (its size and SHA-256), and
writes `DotAny-Setup.exe` (`dotany-setup.exe` with the files appended, zstd level 19 unless `--level` says otherwise)
and `manifest.json` (the release's version and every file's SHA-256) to `--out`, by default
`<build>/cargo-target-installer/`. It prints the sizes. If the hub, any program, `libmp3lame.dll` or a font isn't
there, or a program's version can't be read, it lists every missing piece, writes nothing and exits with code 2, so an
installer never says it installed something it doesn't carry.

`payload.json` says what an installer carries: the components in the order of the installer's cards, the files of
each (`from` relative to the checkout, or `{build}`), the types each tool registers (`formats`), the three `claim`
lists, and the right-click entries the programs may write themselves (`cleanup`), which the uninstaller removes. The
version of each program is read from its `tauri.conf.json`.

The installer is not code-signed. A signature added to it goes after the payload: the reader looks for the payload's
footer near the end of the file, not at the very end, so a signed installer still works. `uninstall.exe` is cut before
the payload, so it would need signing on its own.

## Where things are

| Part | What |
|---|---|
| `src/` | The page: `index.html`, `styles.css`, `main.js`, and `lang/` with its words in each language (Chinese is the source). |
| `src-tauri/src/lib.rs` | How the program runs: installer, quiet install, update mode or uninstaller; WebView2. |
| `src-tauri/src/app.rs` | The window and the commands the page calls. |
| `src-tauri/src/engine.rs` | Stage, commit, roll back; uninstall; start with Windows. |
| `src-tauri/src/reg.rs` | Every registry key written, all under HKCU. |
| `src-tauri/src/system.rs` | Folders, shortcuts, running programs, free space, fonts. |
| `src-tauri/src/payload.rs` | The payload's layout and the reader. |
| `src-tauri/src/manifest.rs` | What a release carries (`manifest.json`) and what an install leaves (`install.json`). |
| `src-tauri/src/words.rs` | The language, and the few words said before there is a window. |
| `src-tauri/src/bin/pack.rs` | `dotany-pack`. |
| `payload.json` | What an installer carries. |

Every place the installer writes to has an environment variable that moves it (`DOTANY_SETUP_HIVE` for the registry,
and others in `system.rs`), so the tests never touch the real keys, Start menu or folders.
