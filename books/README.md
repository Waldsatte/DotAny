# DotAny Books

DotAny Books opens an e-book or a comic and lets you read it: pages that turn over or a scroll, a contents list, notes
where you are, find across the whole book, page numbers and bookmarks. A book is only ever read; nothing is unsaved, so
closing never asks. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with
esbuild) and a Rust program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on the Mac.

## What it opens and saves

| Format | Extensions | Notes |
|---|---|---|
| EPUB | `.epub` | Without DRM. Obfuscated fonts are not a lock. |
| FB2 | `.fb2`, `.fbz`, `.fb2.zip` | One XML file, or the same inside a ZIP. |
| MOBI | `.mobi`, `.prc` | PalmDOC and HUFF/CDIC compression, without DRM. |
| Plain text | `.txt` | Chapters found by their headings. UTF-8, UTF-16 with a byte order mark, or GB 18030. |
| Comics | `.cbz`, `.cbt`, `.cb7` | Pictures in a ZIP, a tar or a 7z. |
| DjVu scans | `.djvu`, `.djv` | Decoded a page at a time; searched where the file has a text layer. |

A book is saved only as a copy of the file, in its own format and byte for byte. A book's kind comes from its extension,
and its bytes are then checked against it.

Not opened: KFX, AZW3, CBR and CBA, a password-protected CBZ or CB7, and any book locked by DRM. A `.txt` file opens
here when asked, though double-clicking one belongs to DotAny Text.

## What you can do

Open a book from the dialog, by dragging it in, or from the file manager. Turn pages by clicking the left or right
side of the window, by the arrow, Page Up/Down and space keys, by the wheel or by a trackpad swipe; Home and End go to
the start and the end. Read as turning pages (one or two columns, depending on the window) or as one scroll from
chapter to chapter. Choose a serif or a sans typeface, one of five sizes, three line spacings and three widths for the
text, and fit the whole page or the width for comics and scans. Open the contents from the top bar. Click a footnote to
read it where you are. Find (`Ctrl+F`) searches the whole book. Number the book's pages, go to one (`Ctrl+G`), drag
along the bar, and keep bookmarks (`Ctrl+D`). A book reopens where you left it. The words are in nine languages.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-books.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the
family's, `shared/scripts/build.mjs`, with Books' own additions in `scripts/build.mjs`: the reading serif's pieces and
the DjVu decoder are copied into `dist/`, and the licences of both are added to the notices. `src/icons.js` is written
by the page build from `lucide-static`, and `src/notices.generated.js`, the licence notices the program shows, by
`shared/scripts/notices.mjs`. `DOTANY_NO_FONTS=1` leaves the serif pieces out of the build. The Mac's application and
its `Info.plist` and `lproj/` folders are made by `shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Books itself (`TOOL`), the book open in each window, and the commands the page calls. It looks at
  a file's bytes (`judge`) and refuses a locked book before the page sees it.
- `comics.rs`: the pictures of a CBT (tar) or a CB7 (7z), handed to the page as a plain, uncompressed ZIP.

**Shared with the rest of DotAny:** `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files,
the watcher that notices when another program changes the file, fonts and the platform layer.

**The page** (`src/`):

- `main.js`: the window, the layout, turning, the contents, the type panel, find, page numbers and bookmarks.
- `readers.js`: each format into a book: chapters of clean HTML (`readEpub`, `readFb2`, `readMobi`, `readTxt`) or a list
  of pictures (`readCbz`, `readDjvu`).
- `find.js`: the matching: folding the typed words and the page's words alike, and turning a match into a range on the
  page or boxes over a scan.
- `backend.js`: the calls to the Rust side. `lang/`: Books' words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, the text
  caret, keys, trackpad gestures, sound, and the family's words and licences. `shared/web/family.css` comes first in the
  built stylesheet; `src/styles.css` only adds to it.

**Beside them:** `fonts/` (the reading serif, its licence and the pieces it is cut into), `scripts/split-serif.py`
(cuts the serif into pieces), `vendor/djvujs` (the DjVu decoder), `src-tauri/windows-app-manifest.xml`.

## How it works

**The Rust side looks first.** `open_file` reads the file, names its format from the extension and checks the bytes
against it: a ZIP that opens and has what an EPUB must have, a PalmDB header for MOBI, `AT&TFORM` for DjVu, the 7z
signature for CB7, a tar header's checksum for CBT. A locked book is refused with its own message and never reaches
the page. A CBT or CB7 is unpacked there and the page reads its pictures as a ZIP. The bytes then go to the page, which
tells the Rust side when it has read them (`book_opened`); only then does the book become the window's open book,
watched and on the recent list.

**The page lays the book out.** A text book is a list of chapters, each clean HTML. In page mode a chapter is set in
CSS columns of a readable width and shown by moving the columns sideways; a window wide enough shows two. A turn lays
the next sheet over the old one, or draws the top one away, so the screen is never blank. In scroll mode chapters are
added above and below as you reach them. A comic or a scan is a list of pictures shown one at a time. The page's language
is set from the book (English for a Latin-script book, Chinese otherwise), which sets its hyphenation and line breaks.

**The reading serif is Books' own.** Noto Serif SC (SIL OFL 1.1, the Simplified Chinese subset of the variable font,
one file for every weight, 10.4 MB as WOFF2) is in `fonts/`. `scripts/split-serif.py` cuts it by character into eleven
WOFF2 files in `fonts/serif/` and writes their `@font-face` rules, each with its `unicode-range`, to `fonts/serif/faces.css`.
Piece 0 is everything that is not a Han character (Latin, digits and every punctuation mark, kept together so marks
shaped side by side stay in one font), piece 1 the 3,755 characters of GB 2312 level 1, piece 2 level 2, and the rest in
runs by code point. Every glyph, weight and layout feature is kept, so a page set in the pieces looks the same as one
set in the whole font. The pieces and `faces.css` are committed; run the script again only when the font changes (it
needs `fonttools` and `brotli`). The build copies the pieces into `dist/fonts/serif` and adds `faces.css` after the
tool's sheet, so they are compiled into `dotany-books.exe`; the whole font itself is not. Nothing of it is read for the
empty window. When a text book opens, `main.js` loads the pieces that the book's characters are in (title, text,
contents and notes) before laying it out, so no page waits on a piece and the page a book reopens on is counted in its
final face. A Chinese book reads pieces 0 and 1, sometimes 2. The serif is first in `--read-serif`, so a computer's own
Noto Serif SC, Source Han Serif or SimSun is never used instead. Its licence is in the licences list in the About
dialog.

**Page numbers.** A page is a fixed amount of text: about 500 Chinese characters or 1,600 letters of a Western book,
with every chapter starting on a new page. The number depends on the words only, never the window, the type or the
reading mode. Comics and scans count their real pages. The page shown is the one the screen starts on, found from the
words themselves (`caretRangeFromPoint` at the corners of the page), so a new layout that keeps the same words on
screen keeps the same page; a page you went to (by number, the bar or a bookmark) stays the page shown while its words
are on screen.

**Find** searches each chapter's text, read from the same HTML the page shows and folded alike on both sides: lower case,
compatibility forms (full width, ligatures) and accents taken off, so `ａ` finds `a` and `é` finds `e`, while the kana's
voicing marks stay. A paragraph is one run of text, so a word split by bold or a link is still one word; a space or line
break beside a Chinese or Japanese character is dropped, so a sentence broken across lines is still found. A match
never runs from one paragraph into the next. The search starts from the chapter you are in, goes to the end and then
from the start, in small pieces between which the window answers, and the count grows as it goes. The marks are the CSS
Custom Highlight API (`::highlight(find-hit)` and `::highlight(find-cur)`), so the page's HTML is never changed. On a
scan the marks are boxes over the words of its text layer. A comic, or a scan without a text layer, has nothing to find
and no find button; whether a scan has words is read before the book is shown, so the top bar never changes once it is
on screen. Each chapter's folded text is kept only while find is open.

**Where you stopped** is kept per book, by its name and size, in the tool's settings (`dotany-books-at`, the newest
300), so a moved copy still opens where you left it. Bookmarks (`dotany-books-marks`, the newest 300 books) are a page
and a place in the words, listed in the contents panel's second tab, and the layout choices (`dotany-books`) are kept the
same way.

## What it is careful about

- **Locks.** A book is looked at for a lock before the page gets it: MOBI's encryption field (record 0, byte 12), an EPUB's
  `META-INF/rights.xml` or `license.lcpl`, or an `encryption.xml` that encrypts anything beyond fonts. A locked book is
  refused with a message, never reaches the page and is not remembered. Nothing is ever decrypted: the `zip` crate is
  compiled with deflate only, there is no decryption code in the program, and a password-protected CBZ or CB7 is refused
  as unreadable. The page checks again as a second look.
- **Size.** A ZIP's declared sizes are not trusted: deflate cannot shrink anything more than about 1032 to 1, so an
  entry claiming more than that is refused, and a book may unpack to at most 8 times its file (256 MB for any small one,
  never over 2 GB). The same limit applies to a CBT or CB7 on the Rust side. An EPUB's `encryption.xml` is read up to
  1 MB.
- **What survives.** Only plain reading content: no scripts, styles, forms, media or links; pictures are `blob:`
  addresses, let go when the book closes. The window's content security policy allows scripts and styles only from the
  program itself, and the DjVu decoder is fetched from the program, never from the network.
- **The file is never changed.** Save As writes the original bytes, and a CBT or CB7 is saved as it came, not as the ZIP
  the page read.
- **Layout changes keep your place.** Changing the size, the width or the typeface keeps you on the sentence you were
  reading, and a window that is resized does the same.

## Windows and the Mac

- **Windows.** Each book opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons, and the top slides away while you read (the top edge or the bottom edge brings it
  back). `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows requires before it shows the snap
  layouts over the maximise button.
- **The Mac.** One program shows each book in a window of its own. Where each book was left and its bookmarks are the
  program's settings, one list for every window: each window's page keeps a copy and hears another window's change as it
  is made, so neither puts back what the other had before. In a window the top stays and the book starts below it,
  because the system's window buttons are not part of the top and cannot go with it; it steps aside only in full screen,
  and comes back for a moment when full screen starts. A trackpad swipe turns one page, however long its momentum
  lasts. `⌘↑` and `⌘↓` go to the start and the end of the book. Escape puts away what is open (the page box, find, a
  note); with nothing open it goes to the system, which leaves full screen.
