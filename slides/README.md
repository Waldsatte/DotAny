# DotAny Slides

DotAny Slides opens a presentation, shows it, lets you change it and play it, and leaves the file alone until you save.
It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with esbuild) and a Rust program
in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon.

## What it opens and saves

| Format | Opens | Can be saved as |
|---|---|---|
| PPTX | yes, and can be edited | PPTX, PDF, PNG |
| PPSX (slide show), POTX (template) | yes, and can be edited | the same format, PDF, PNG |
| PPTM, PPSM, POTM (with macros) | yes, view only; macros are never read or run | PPTX (the macros stay behind), PDF, PNG |

A file is what its first bytes say: a ZIP, or a compound file, which is a presentation with a password and is refused
with a notice. The old binary PPT is not opened: no code for it is compiled in. A PDF or a PNG is a copy of what is shown,
so the window stays on the presentation.

## What you can do with a presentation

Look at the slides one at a time or as a grid of pages; add, duplicate, hide, delete and drag pages into a new order.
Select, move, resize and turn what is on a slide, and change its words in place (size, bold, italic, underline,
alignment, bullets, colour), its fill, its line and its stacking order. Add a text box, a picture (PNG, JPG, GIF, BMP),
a shape, or a film or sound (MP4, M4V, MOV, WMV, AVI, MP3, WAV, M4A, WMA, OGG, FLAC). Set a slide's transition and the
effects of its shapes in the motion panel. Write the speaker's notes. Read the file's facts in the Info panel (title,
author, number of slides, aspect ratio, size, animations, films and sounds). Undo and redo. Play the show in a full
screen with its animations, transitions, films and sounds, with a laser pointer and a black screen, and a presenter's
view with the current slide, the next one and the notes. Print the chosen slides to a PDF, one to a sheet the slide's own
size, at the original quality or compressed, or save them as PNG pictures 1280, 1920 or 3840 pixels wide, several
numbered beside the chosen name (`name-01.png`).

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-slides.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the
family's, `shared/scripts/build.mjs`; it also writes `src/icons.js` from `lucide-static`. `src/notices.generated.js`, the
licence notices the program shows, is written by `shared/scripts/notices.mjs`. The Mac's application and its
`Info.plist` and `lproj/` folders are made by `shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Slides itself (`TOOL`), reading and adopting a presentation, writing the bytes the page made,
  printing to a PDF, the open dialog for pictures and films, links, full screen, and Windows playing films and sounds.
- `media_foundation.rs`: Windows' Media Foundation playing a film or sound and sharing each picture with the page.
  `media_macos.rs` answers the same calls on the Mac, where nothing is played this way.
- `mac.rs`: what the Mac does instead of WebView2 and Media Foundation: WKWebView printing the PDF and taking pictures,
  an AVI repackaged for WKWebView, and the screens, for the show and the presenter's window.

**Shared with the rest of DotAny:** `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files,
the watcher that notices when another program changes the file, fonts and the platform layer. `dotany-repack`
(`../shared/repack`) repackages an AVI on the Mac.

**The page** (`src/`):

- `main.js`: the window, the slides, the dock, the panels, editing, the show and the save card.
- `deck.js`: reading a presentation (the parts, how a slide inherits from its layout and master, the theme, notes, click
  steps and transitions). `paint.js`: drawing a slide as real elements, with the text as text. `chart.js`: the common
  charts. `edit.js`: changing the file. `picture.js`: a slide as a PNG.
- `system-film.js`: a film or sound that Windows plays, drawn on a canvas. `presenter.js`: the presenter's window on the
  Mac. `backend.js`: the calls to the Rust side. `lang/`: Slides' words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, fonts,
  keys, sound, and the family's words and licences. `shared/web/family.css` comes first in the built stylesheet;
  `src/styles.css` only adds to it.

## How it works

**Reading.** Rust hands the page the file's bytes; `deck.js` unzips them with fflate and reads the parts. Only then does
Rust adopt the file (watcher, recent list), so a file that cannot be read leaves the open one alone. A slide is drawn
once, as real elements, and scaled to whatever size is needed; the pages view and the show draw it again from the same
paper.

**Saving a PPTX.** Every part that was not edited is written back with its original contents (`saveDeck` in `edit.js`);
only the parts that were edited are serialised again, so what the program does not understand stays as it was. Pictures,
films and sounds are stored without further compression, being compressed already. A PPSX or POTX is saved as itself: the save card's first format
is the file's own kind.

**Macro files.** A PPTM, PPSM or POTM is only read: the window says so in a notice row and editing is off. Save As offers
PPTX; the VBA project, its signatures and what points at them are removed, the main part's content type becomes a
presentation, and the window moves to the new PPTX, which can be edited (`dropMacros` and `setMainType` in `edit.js`).

**PDF.** On Windows it is WebView2's own printing (`ICoreWebView2_7::PrintToPdf`): the chosen slides are painted into a
print layer, one to a sheet the slide's own size, with no margins, and printed. The compressed sizes redraw pictures
smaller (never below what is shown) as JPEG.

**PNG.** On Windows it is drawn in the page: the painted slide, its pictures inlined, as an SVG picture onto a canvas at
the chosen width. On the Mac WebKit draws such an SVG picture wrongly, so the slides are printed as for a PDF and each
sheet is drawn by CoreGraphics at the width asked (`pdf_pictures` in `mac.rs`).

**Films and sounds** play where they sit on the slide. MP4, M4V, MOV, MP3, WAV, M4A, OGG and FLAC play in the page from
the file's own bytes; WMV, AVI, WMA and ASF go to Windows' Media Foundation, from a temporary copy that is deleted when
playing stops, and arrive as pictures in memory shared with the page. There is no decoder of its own.

**The show** is the window itself going full screen. On a Mac with more than one screen the show goes to the screen the
audience sees and the presenter's view opens in a window of its own on the Mac's screen (`presenter_open`,
`mac::open_presenter`); that window keeps no deck: every key and click goes back to the show's page, which sends the new
state.

## What it is careful about

**What was not edited does not change.** See Saving a PPTX above. Untouched parts, including pictures, films and
sounds, are carried over as they came.

**Nothing in a file is run.** Macros are neither read nor run; a link is opened by the system's browser, and only if it
is an http or https address.

**Large parts.** A part of the ZIP that is 512 MB or more once unpacked is skipped when the file is read.

**Saving.** A PPTX is written to a temporary file in the same folder, flushed, then swapped in, so a failed save leaves
the old file whole. A PDF or a PNG is written the same way and does not go on the recent list.

## Windows and the Mac

- **Windows.** Each presentation opens in a program of its own, with one window. The window has no system title bar: the
  page draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button. Films and sounds that WebView2 will not play are
  played by Media Foundation, which is looked up when first needed, so a Windows without it still starts the program and
  such a film is refused with a notice.
- **The Mac.** One program shows each presentation in a window of its own. WKWebView prints the PDF, takes the pictures
  and plays the films it can; an AVI is repackaged into a film it plays, and WMV and WMA are not played (the page says
  so). The show can use a second screen, as above. The ⌘ keys are the menu bar's even during a show.
