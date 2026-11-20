# DotAny PDF

DotAny PDF opens a PDF, lets you read it, mark it, change its words, rearrange its pages and save the result, and
leaves the file alone until you save. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain
JavaScript, built with esbuild, drawing the pages with [PDF.js](https://mozilla.github.io/pdf.js/)) and a Rust program
in `src-tauri/` that writes the files. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon.

## What it opens and saves

| Format | Opens | Can be saved as |
|---|---|---|
| PDF, including files that need a password | yes | PDF, Word (`.docx`), and PNG or JPG for the page on screen |
| JPG, PNG | as a page added to an open PDF (from the pages view) | inside the PDF they were added to |

A file with a password asks for it; a wrong one asks again. JPEG 2000 pictures inside a PDF are read with OpenJPEG, in
PDF.js's worker. HTJ2K (JPEG 2000 Part 15) is left out on purpose: `src/jpx-guard.js` refuses such a picture the way a
damaged one is refused, and the rest of the page draws. Scripts in a file never run, a web address in a file is only
offered (it opens in the system's browser when you say so, and only for `http` and `https`), and XFA forms are not drawn.

## What you can do with a PDF

Read it with zoom, a page box, a table of contents and find. Select, copy and look at its details. Fill in its form.
Highlight, draw with a brush, add notes, sign (by drawing, or with a picture of a signature), black out parts so that
nothing under them is left, and change a paragraph's words in place. In the pages view, turn, delete, reorder (by
dragging) and extract pages, add blank pages, and add pages from other PDFs and from JPG and PNG pictures. Undo and redo
cover all of it. A file reopens at the page where you left it.

When saving, the file's size is a choice of its own: the original quality, or the pictures inside written again as
smaller JPEGs (two levels).

A file that limits what may be done (no changes, no copying and so on) turns the matching tools off, and a notice says
so. The file's owner password lifts the limits; the password to open it does not. When copying is not allowed, Word, PNG
and JPG are not offered either.

**Not done:** OCR, certificate signatures, XFA, and printing.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-pdf.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is
the family's, `shared/scripts/build.mjs`; this folder's `scripts/build.mjs` adds PDF.js's worker (with the HTJ2K guard
in it), its character maps and its standard fonts, and the licence notices of OpenJPEG and the standard fonts.
`src/icons.js` is written from `lucide-static`, and `src/notices.generated.js`, the licence notices the program shows,
by `shared/scripts/notices.mjs`. The Mac's application and its `Info.plist` and `lproj/` folders are made by
`shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes PDF itself (`TOOL`), what each window holds (its open file, the PDFs and pictures added to it,
  the pictures its page drew for a save), and the commands the page calls.
- `save.rs`: writing the PDF with [lopdf](https://crates.io/crates/lopdf) from the plan the page sends.
  `textcut.rs`: taking a changed paragraph's old words out of a page's content. `embed.rs`: new words in the family's
  fonts, embedded as subsets. `shrink.rs`: the smaller sizes. `docx.rs`: Word. `geom.rs`: PDF's matrices.

**Shared with the rest of DotAny:** `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent
files, the watcher that notices when another program changes the file, fonts and the platform layer.

**The page** (`src/`):

- `main.js`: the window, the pages, the tools, find, the contents, the pages view and the plan each save sends.
  `backend.js`: the calls to the Rust side. `links.js`: which web addresses may be opened. `jpx-guard.js`: the HTJ2K
  check. `lang/`: this tool's words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings, the Save As card, the notice
  row, fonts, the text caret, keys, trackpad gestures, sound, and the family's words and licences.

## How it works

**PDF.js reads and draws; Rust writes.** The page keeps every mark (highlights, brush strokes, notes, signatures,
redactions, changed paragraphs) in the page's own coordinates, with one undo list for everything that changes the file.
A save sends Rust a plan: which pages in which order (from the open file, from other PDFs, from pictures, or blank), how
each is turned, what was drawn or written on it, the form's values and the size asked for.

**What a save writes.** If nothing changed and the original quality is asked for, the original's bytes are the file.
If the pages are the file's own, in its own order, with a sound cross-reference table, the save is an incremental
update: the original bytes come first, untouched, and only the changed objects are appended. Anything else is a new
file, written whole, in which nothing that no page uses is left. A file that was encrypted is encrypted the same way
again.

**Changed paragraphs.** Paragraphs are worked out from where each word sits (same size, close together, overlapping
sideways). A changed paragraph's old words are cut out of the page's content, everything after them on the line staying
put, and the new words are drawn on top in the family's fonts, embedded as subsets with a ToUnicode map so they can be
found and copied again.

**Redactions.** A page with a redaction is drawn whole as a picture (200 dots to the inch) with the covered part black,
and that picture replaces the page: its content, annotations and fields go, and so does the document's tagged
structure, which can carry the covered words as text.

**Memory.** A drawn page holds width × height × 4 bytes of graphics memory, so only the pages near the view stay drawn
(about 16 million pixels beyond the ones on screen). A page further away gives its canvas back, and PDF.js its pictures
and words, and keeps a small picture of itself for the moment it takes to be drawn sharp again.

## What it is careful about

- **The file stays as it was until you save.** Saving is Save As: a name and a format, and the system's save dialog asks
  before replacing a file. The file is written to a temporary file in the same folder and swapped in, so a failed save
  leaves the old file whole; the edits stay and the window says why. A file that lopdf cannot read can still be saved
  as an untouched copy, and nothing else.
- **A password is kept in memory only**, for the open window.
- **Size is checked before pixels are made.** A PNG added as a page says how big it is in its header, and one
  claiming more than 80 million pixels is refused before anything is decoded. A JPEG is put into the file as it is.
- **Only what a file allows.** The file's permissions turn the matching tools and the Word, PNG and JPG formats off.
- **Web addresses** are offered in a bar over the link; the address is checked again on the Rust side before it is
  handed to the system's browser.

## Windows and the Mac

- **Windows.** Each PDF opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button. A serif paragraph is written in Noto Serif SC
  when Windows has it installed; otherwise in Stack Sans and HarmonyOS Sans SC, like the rest.
- **The Mac.** One program shows each PDF in a window of its own. Two fingers on a trackpad scroll the pages with the
  system's momentum, a pinch zooms about the fingers, and smart zoom (a two-finger double tap) fits the page's width and
  goes back to the earlier zoom. WebKit's canvas has no colour filter, so the page's night colours (when paper dimming
  is on) are worked on the pixels after the page is drawn. The shortcuts use the Mac's keys (⌘↑ and ⌘↓ for the first
  and last page).
