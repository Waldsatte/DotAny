# DotAny Text

DotAny Text opens a text file, a Markdown file, a Word document or a log of any size, lets you read and change it, and
leaves everything it was not asked to change exactly as it found it. It is a [Tauri 2](https://tauri.app) program: a
page in `src/` (plain JavaScript, built with esbuild, with ProseMirror and CodeMirror as the editors) and a Rust
program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon. Each window
holds one document, and one program runs all the windows.

## What it opens and saves

The view a file opens in follows from its name and size; its encoding and line endings follow from its bytes. A file
that holds NUL bytes and is not UTF-16 is not text, and is refused with a notice.

| Format | Opens as | Can be saved as |
|---|---|---|
| Word (`.docx`) | the article view, with A4 pages on request | `.docx`, `.md`, `.txt` |
| Word 97–2003 (`.doc`), RTF, OpenDocument text (`.odt`) | the article view, locked: read only | a new `.docx`, `.md` or `.txt` |
| Markdown (`.md`, `.markdown`) | the article view when the whole file is something it can draw, otherwise the code view | `.md`, `.docx`, `.txt` (as itself when it is in the code view) |
| Plain text (`.txt`) | the article view, one paragraph per line | `.txt`, `.md`, `.docx` |
| Code, data and subtitles: JSON, Python, JavaScript, TypeScript, HTML, CSS, YAML, XML, SVG, TOML, INI, SRT, VTT, and any other text | the code view | the same format |
| `.log`, and any file over 20 MB | the long scroll: read only, any size | a copy of the file's bytes |
| DOS text art: `.nfo`, `.diz`, `.ans` | the art view: read only, in a DOS face; an `.ans` is drawn with its colours | `.txt` (UTF-8, without colours) |

The article view takes a Markdown or plain text file only up to 400,000 characters and 8,000 lines; a longer one goes
to the code view. A text file opens for editing when it is UTF-8 (with or without a byte-order mark), UTF-16 with a
byte-order mark, or GBK, GB18030, Big5, Shift_JIS or EUC-KR and Text is sure of which; a file in any other encoding is
shown read only, and the encoding can be chosen.

## What you can do

- Write and edit in the article view: paragraph styles (headings 1 to 3), bold and italic, lists and tables, pictures,
  and for Word also underline, size, colour, alignment and line spacing. Find and replace across a paragraph's
  formatting, a word count, and a page view that lays a Word file out on A4 sheets. Saving a Word file as Markdown
  writes its pictures beside the new file.
- Edit code and data in the code view: line numbers, syntax colours, folding, find and replace (a regular expression
  if you want one), go to a line, a choice of indent and of wrapping, rectangular selection, and for JSON a format
  command, an outline of the structure and a mark on a syntax error.
- Read a log in the long scroll: it indexes the file in the background, follows a log that is still growing, filters by
  level (all, warning, error) and finds across the whole file.
- Choose the encoding and the line endings a file is saved with (LF or CRLF), and open the file again in another
  encoding when the first guess was wrong.
- Start a new document of a chosen kind (Word, Markdown, text, a source or data file, subtitles), open several windows,
  keep recent files, print, and turn on saving as you type (off by default).
- Start Text with a file as its argument, with `--new` (the choice of kinds) or with `--new=md` (one kind: `docx`, `md`,
  `txt`, `py`, `js`, `ts`, `html`, `css`, `json`, `yaml`, `toml`, `xml`, `ini`, `srt` or `vtt`).

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-text.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). `npx tauri dev`
builds the page and runs a debug build of the program. Setting `DOTANY_NO_FONTS=1` for the page build
leaves the fonts out. The page build uses the family's options (`shared/scripts/build.mjs`) and copies Text's own fonts
(JetBrains Mono for code, Unscii for text art) from `fonts/`. `src/notices.generated.js`, the licence notices the program shows,
is written by `scripts/notices.mjs` from the crates and packages that are really built in. The Mac's application and
its `Info.plist` and `lproj/` folders are made by `shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Text itself (`TOOL`), its windows and the commands the page calls. Text keeps one document per
  window itself, since it is the one program of the family with several windows in one process; it tells the family only
  what it opened and saved.
- `encoding.rs`: finding a file's encoding, decoding and encoding. `document.rs`: the file in memory, and line endings.
- `long.rs`: the long scroll's index, reading, filter and find. `art.rs`: code page 437 and the end of a DOS file.

**Shared with the rest of DotAny:**

- `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files, the watcher that notices when
  another program changes the file, fonts and the platform layer. `../shared/README.md` says how Text sits on it.
- `dotany-words` (`../shared/words`), which the hub's converter uses too: in `src/docx/`, `mod.rs` reads Word files,
  `write.rs` writes them, `package.rs` handles the zip and `xml.rs` keeps exact source positions; `src/import/` reads
  doc (`doc.rs`), RTF (`rtf.rs`) and ODT (`odt.rs`) and builds blocks and the Word document from them (`mod.rs`).

**The page** (`src/`):

- `main.js`: joins the parts and opens the first file. `files.js`: a file's life in a window (which view, the status
  line, saving, notices, printing). `shell.js`: the window's frame from the family's `createShell`, with Text's buttons,
  its format bar and its small menus. `settings`, the Save As card and the new-file panel are the family's, with Text's
  formats in `save-as.js` and `formats.js`.
- The three views: `doc-editor.js` (the article view), `code-mode.js` (the code view) and `long-scroll.js` (the long
  scroll), and `art-view.js` with `ansi.js` for text art. `markdown.js`: Markdown and the article view. `docx-view.js`:
  Word blocks and the editor's document. `doc-find.js`: find and replace in documents. `schema.js`: the editor's
  document shape. `backend.js`: the calls to the Rust side.
- From `shared/web` (`@dotany/web/…`): the window frame, fonts, the text caret, keys, paths, sound, and the family's words
  and licences. `shared/web/family.css` comes first in the built stylesheet; `src/styles.css` only adds to it.
  `src/i18n.js` and `lang/` are Text's own words in each language, added to the family's.

## How it works

**Rust holds the file and the page edits text.** Opening reads the bytes, finds the encoding and the line endings, and
sends the page the text with `\n` endings. The editors work on that text. A save sends the text back; Rust turns it into
bytes with the file's own encoding and line endings and writes them. The bytes that were read stay in memory
(a save replaces them with what it wrote), so another encoding can be tried at any time by decoding them again.

**Which view a file gets** follows from its name and size: `.docx`, `.doc`, `.rtf` and `.odt` go to the Word reader;
`.nfo`, `.diz` and `.ans` to the text art reader; a `.log`, or anything over 20 MB, to the long scroll; a `.md` or `.txt`
of a size the article view takes, to the article view; everything else to the code view, which loads when it is first
needed.

**The long scroll** keeps an index of one byte offset for every 1024 lines, built in the background, so a 500 MB file
with ten million lines needs about 80 KB. Only the lines on screen exist in the page; Rust reads them from the disk as
they are needed. The scroll position is a line number plus how far into that line, because browsers cap an element's
height near 33 million pixels, so the scrollbar is proportional while the wheel moves real pixels.

**Another program changing the file.** A watcher tells the window when the file's size or time changes. If the new bytes
are the same as what was read, nothing is said. A log that grew is read on from where it stopped. Otherwise the window
asks whether to read the new version or to keep its own, and a save over a changed file stops until it is answered.

## What it is careful about

**Encoding.** A file opens for editing only if its encoding is certain and re-encoding the decoded text gives back the
same bytes. Otherwise it opens read only with a notice, and the encoding can be chosen from a menu. "Certain" means a
byte-order mark, valid UTF-8, or a legacy guess (GBK or GB18030, Big5, Shift_JIS, EUC-KR) with at least 40 non-ASCII
bytes, no decoding errors and text that reads as ordinary CJK. If a reading cannot hold all the bytes, editing it is not
offered. Saving in an encoding that cannot write a character stops and names the characters; nothing is substituted.

**Line endings.** The editors work in `\n`. A file with one kind of ending keeps it for every line. With mixed endings,
the new text is compared with the old line by line on saving: an unchanged line keeps its own ending, a replaced line
keeps the ending of the line it replaces, and a new line takes the ending of the line above (at the very top, the
file's most common ending). Only choosing LF or CRLF in the status line rewrites the other lines. A file with no line
break that gains one gets LF.

**Saving.** The file is written to a temporary file in the same folder, flushed to disk, then swapped in, so a failed
save leaves the old file whole. The swap keeps the file's attributes, permissions and creation time. A shortcut or
symbolic link keeps pointing at the real file, which is what is replaced. If the bytes would not change, nothing is
written. If the disk holds something other than what was read, the save stops and says so. Save As in the file's own
format moves the window to the new file; Save As in another format writes a new file and leaves the window on the
original.

**Markdown.** Every top-level block remembers the exact text it came from. Saving writes that text back for every block
that is unchanged, and new Markdown only for blocks that were edited, so the bytes of everything else stay as they
were, whatever style the file was written in. The article view draws headings 1 to 3, one-line paragraphs, bold and
italic, lists, simple tables and pictures alone on a line. Inline links and bare addresses are shown and edited as the
text they are, character for character. Anything else (code blocks, quotes, reference links, HTML, front matter, hard
line breaks and so on) sends the whole file to the code view. A picture whose path names another computer or a device
(`\\host\share\…`, `//host/…`, `\\?\…`, `\\.\…`) is not read, because merely looking at such a path makes Windows
connect and sign in to that server; it shows as a missing picture. The one exception is a document opened from a
network share: pictures on that same `\\server\share` show as they would on a local disk.

**Word.** A paragraph, table or picture the editor can represent is read into a block that remembers its exact XML.
Saving rewrites only the blocks that changed and copies every other part of the package without recompressing it. A
block holding something that cannot be edited (a field, a link, a footnote reference, a text box, an equation) is shown
locked: it cannot be deleted or typed over, and stays byte for byte. If the file as a whole cannot be kept exactly
(tracked changes switched on, editing protection, the strict format, an unusual layout) it opens read only and offers
Save As, which copies the bytes. If nothing changed, nothing is written. A picture never goes inside a table cell: it
goes after the table.

**doc, RTF and ODT** are read, never written. A reader turns the file into the editor's blocks, those become a Word
document in memory, and the window shows it locked, with a notice that offers Save As. Save As writes that Word
document, as shown, as a new `.docx` and the window moves to it; Markdown and plain text are offered too and leave the
window where it is. Which reader is used follows from the file's first bytes: an RTF or a Word 2007+ file named `.doc`
opens too, and is just as read only. A file no reader can take is refused with a notice.

What comes across, in all three: paragraphs and their words; bold, italic, underline, size and colour; alignment, line
spacing as a multiple, spacing before and after, indents; headings 1 to 3; bulleted and numbered lists, nested;
tables; PNG, JPEG and GIF pictures; tabs, line breaks, and the words a field shows (a date, a page reference, a table of
contents). A picture inside a line of words stands on a line of its own between them. In a list whose levels mix bullets
and numbers, each kind becomes its own Word list.

What does not come across, and makes the notice say that some content cannot be shown and will not be in the new file:
footnotes and endnotes, comments, text boxes and shapes, embedded objects and equations, pictures in other formats (WMF,
EMF, BMP, TIFF), a picture in a table cell, links (the words stay, the address does not), symbols from symbol fonts,
merged and nested table cells (a nested table's words stay in the outer cell), tracked changes (the text as it now reads
is kept), headers and footers with words in them, and rows shorter than the others (filled out with empty cells). Left
out without a notice, as formatting: fonts, highlighting, strikethrough, small and all capitals, superscript and
subscript, heading levels 4 and below (plain paragraphs), exact line heights, page size, columns and sections. Hidden
text is not shown, as in Word.

- **doc**: Word 97–2003, read through the file's piece table, so fast-saved files read too. Word 6 and 95 (an older
  layout), encrypted files and files with a damaged container are refused. The rare one-property form of a piece's own
  properties (Prm0) is not applied. It uses the `cfb` crate for the container.
- **rtf**: Word's list tables and the older `\pn` numbering (WordPad), headings from the style sheet, code pages by each
  font's character set (`\fcharset0` is Windows-1252), `\u` with its fallback skipped, pictures as hex or `\bin`.
- **odt**: `content.xml` with the named styles of `styles.xml`, list styles by level, sections and indexes as their
  words, text hidden by its style left out. Encrypted packages are refused. Repeated rows and columns stop at 200.

**Sizes.** Above 20 MB, and for every `.log`, the file opens in the long scroll. In the long scroll a line longer than
64 KB is shown cut short; the file is not touched. doc, rtf and odt have no size limit.

**DOS text art.** `.nfo`, `.diz` and `.ans` files are written in IBM code page 437, which the encoding library does not
have, so its 256 characters are in `art.rs`. The text stops at DOS's end of file, a Ctrl-Z that ends the file or comes
right before an artist's record (SAUCE, with its comments COMNT); a Ctrl-Z anywhere else is CP437's arrow and is drawn
as one. A file that is already Unicode is read as UTF-8. An `.ans` is drawn on an 80-column screen with its colour and
cursor codes; nothing else in it runs, and the screen cannot grow without bound.

## Windows and the Mac

- **Windows.** Each window is a window of the one program; the window has no system title bar, and the page draws the
  top bar and its buttons. `windows-app-manifest.xml` asks for version 6 of the common controls, which the file dialogs
  use, and declares Windows 10 and 11 support, which Windows requires before it shows the snap layouts over the maximise
  button. Saving swaps the file in with `ReplaceFileW`.
- **The Mac.** One program shows every window, and a new window opens cascaded from the one in front. Files opened in
  Finder, or with the Dock icon, come to the program as they arrive: a file that is already open brings its window
  forward, an empty window takes the file, and any other opens in a window of its own. `Info.plist` lists Markdown, JSONC,
  TOML, INI, JavaScript modules, TypeScript and JSX, subtitles and text art as the types Text is the default for, and plain text,
  logs, Word, RTF, OpenDocument text, JSON, YAML, XML, Python, JavaScript, HTML and CSS as the types it also opens.
  The open dialog is the family's sheet on the window. Find and replace is Option-Command-F, because Command-H hides
  the program. In the long scroll the trackpad's own momentum is used, and Command-Up and Command-Down are Home and End.
  A save is forced to the disk itself before the name changes, and it keeps the old file's extended attributes (Finder
  tags and comments) and creation date.
