# DotAny Sheets

DotAny Sheets opens a spreadsheet, lets you look at it and change cells, and leaves the file alone until you save. It is
a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with esbuild) and a Rust program in
`src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon.

## What it opens and saves

| Format | Opens | Can be saved as |
|---|---|---|
| XLSX | yes, and can be edited | XLSX, CSV, TSV |
| CSV, TSV | yes, and can be edited | CSV, TSV, XLSX |
| XLSM | yes, view only | XLSX (the macros stay behind) |
| XLS, ODS, XLSB, FODS | yes, view only | XLSX, CSV, TSV |
| DBF (dBase, FoxPro, a shapefile's table) | yes, view only | XLSX, CSV, TSV |
| SQLite databases (`.sqlite`, `.sqlite3`, `.db`) | yes, view only | XLSX, CSV, TSV |

A file is what its first bytes say, not what its name says. A ZIP with `xl/workbook.xml` is XLSX, or XLSM when it carries
`vbaProject.bin`; a ZIP with `xl/workbook.bin` is XLSB; a ZIP with `content.xml` and `mimetype` is ODS; a compound file
is XLS; a file that starts with SQLite's header is a database, but only under a database's name (`.db` is also
Windows' thumbnail cache). Text under the name `.xls`, which many systems export as plain tab- or comma-separated text,
opens as TSV or CSV, view only. A workbook with a password (a compound file that holds `EncryptionInfo`) is refused with
a notice, and so is anything else that is not a spreadsheet. A view-only file can be saved as XLSX to edit it.

## What you can do with a spreadsheet

Move a frame from cell to cell and type into it; fill by dragging the corner; jump with Ctrl and an arrow; copy down or
right (Ctrl+D, Ctrl+R); sum what is above or beside a cell (Alt+=); cycle a reference's `$` in a formula (F4); copy and
paste to and from any other spreadsheet. Insert and delete rows and columns, fit a column to its data, drag a column's
edge to set its width. Filter and sort a column, which changes what you see and never the file. Find and replace.
Switch between the sheets of a workbook. Treat the first row as a header. Read a cell's contents or, for a range, its
sum, average and count in the dock. Undo and redo. Open the file's facts in the Info panel (format, sheets, rows, columns,
size, charts, pivot tables, macros, and for a text table its delimiter and encoding). Start a new workbook.

CSV and TSV files are read again in another encoding (UTF-8, GBK, Big5, Shift JIS) from the Info panel, which fixes a
garbled file; saving writes the encoding you choose. A filtered and sorted view can be saved as a copy of what is shown.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-sheets.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the
family's, `shared/scripts/build.mjs`; it also writes `src/icons.js` from `lucide-static`. `src/notices.generated.js`, the
licence notices the program shows, is written by `shared/scripts/notices.mjs`. The Mac's application and its
`Info.plist` and `lproj/` folders are made by `shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/app.rs`): what makes Sheets itself (`TOOL`), the file open in each window, the format of
a file by its bytes (`judge`), and the commands the page calls: `open_file`, `sheet_bytes`, `sheet_model`, `sheet_text`,
`sheet_opened`, `sheet_failed`, `sheet_new`, `save_xlsx` and `save_text`.

**Shared with the rest of DotAny:**

- `dotany-table` (`../shared/table`), which the hub's converter uses too: `text.rs` (which encoding a CSV or TSV is in,
  and writing it back), `package.rs` (what a ZIP is, and writing an XLSX that copies every part it does not rewrite),
  `legacy.rs` (XLS, XLSB, ODS and FODS, through calamine), `dbf.rs` (its own DBF reader) and `sqlite.rs`.
- `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files, the watcher that notices when
  another program changes the file, fonts and the platform layer.

**The page** (`src/`):

- `main.js`: the window, the table and everything done to it. `sheet.js`: what a sheet is, parsing and writing
  delimited text, and the formula engine. `xlsx.js`: reading and writing the XML of an XLSX package. `backend.js`: the
  calls to the Rust side. `lang/`: Sheets' words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, fonts,
  keys, sound, and the family's words and licences. `shared/web/family.css` comes first in the built stylesheet;
  `src/styles.css` only adds to it.

## How it works

**Opening.** The Rust side reads the file and works out what it is. XLSX and XLSM reach the page as bytes, and the page
reads them (`xlsx.js`), keeping every cell's own XML beside its value, its style and its row's attributes; shared
formulas are worked out for every cell they cover. XLS, XLSB, ODS, FODS, DBF and SQLite are read in Rust into cells
(`sheet_model`). CSV and TSV are decoded in Rust (`sheet_text`): a byte order mark, else valid UTF-8, else the guess of
`chardetng`. The file becomes the window's open file (watched, on the recent list) only once the page has read it, so a
file that cannot be read leaves the open one alone.

**Formulas.** `sheet.js` parses a formula into a tree and works it out, so `IF` and `IFERROR` only look at the side they
take. About eighty functions are known: arithmetic and comparison; `SUM` `AVERAGE` `COUNT` `COUNTA` `COUNTBLANK` `MIN`
`MAX` `MEDIAN` `PRODUCT` `LARGE` `SMALL` `SUMPRODUCT`; `ROUND` `ROUNDUP` `ROUNDDOWN` `INT` `ABS` `MOD` `POWER` `SQRT`
`EXP` `LN` `LOG10` `PI`; `SUMIF(S)` `COUNTIF(S)` `AVERAGEIF(S)` `MAXIFS` `MINIFS`; `VLOOKUP` `HLOOKUP` `INDEX` `MATCH`
`XLOOKUP` `CHOOSE`; `IF` `IFS` `IFERROR` `IFNA` `AND` `OR` `NOT`; `CONCAT` `CONCATENATE` `TEXTJOIN` `LEN` `LEFT` `RIGHT`
`MID` `UPPER` `LOWER` `PROPER` `TRIM` `REPT` `EXACT` `SUBSTITUTE` `FIND` `SEARCH` `VALUE` `TEXT` (dates and plain number
patterns); `DATE` `YEAR` `MONTH` `DAY` `WEEKDAY` `TODAY` `NOW`; `ISBLANK` `ISNUMBER` `ISTEXT` `ISERROR` `NA`. Defined
names and other sheets work. A formula that uses anything else keeps the result the file came with.

**Saving an XLSX over an XLSX or XLSM.** Only the sheets that changed are written, and in a changed sheet only
`<sheetData>` is rewritten: every cell nobody changed is its own XML again, at its new address if rows or columns moved.
An edited cell keeps its style; new words are inline strings, so `sharedStrings.xml` is never touched. Every other part
(charts, pivot tables and their caches, drawings, comments, styles, themes, other sheets) is copied from the original
file with its compressed bytes (`raw_copy_file` in `package.rs`). When a sheet changed, `calcChain.xml` is dropped and
`fullCalcOnLoad` is set, so Excel rebuilds its order of calculation and works every formula out again. Every later save
starts again from the file first opened, so what was never edited stays exactly as it was there.

**Rows and columns inserted or deleted** move, besides the formulas in every sheet: merged cells, conditional formats,
data validation, hyperlinks, filters and views in that sheet; defined names; every chart's ranges; every pivot cache's
source; every table's extent. A reference to what was deleted becomes `#REF!`.

**Saving as another format.** XLSM saved as XLSX leaves `vbaProject.bin` and its signature behind and changes the
package's content type. XLS, XLSB, ODS, FODS, DBF, SQLite and CSV saved as XLSX make a new package (`freshXlsx`): values,
formulas with their results, dates and percentages. CSV and TSV (`writeDelimited` in `sheet.js`): a row nobody touched
is its old line again, with its quotes, its line ending and its last line with or without an ending; in a changed row
only the changed fields are written anew, quoted only when they must be. The encoding is the file's own unless another
is chosen. After any save other than a copy of the shown view, the window is on the new file.

## What it is careful about

**What was not edited does not change.** An XLSX is patched, not rewritten (see above), and a CSV or TSV that was read
in its own encoding comes back byte for byte, because the same tables read and write it. A save is written to a
temporary file in the same folder, flushed, then swapped in, so a failed save leaves the old file whole.

**Encodings.** A character the chosen encoding cannot hold stops the save with a notice, and the edits stay in the
window.

**Macros** are never read, run or written. An XLSM opens view only and saves as XLSX without them.

**XLS, XLSB and ODS formulas.** calamine 0.38 garbles XLS formulas that refer to other sheets (it shifts the column and
loses the `$` of a `Sheet!A1`, and takes the wrong sheet for `Sheet!A1:B2`), formulas that use a defined name, and
functions newer than the format; such a cell keeps the result Excel stored and no formula (`xls_formula_trusted` in
`legacy.rs`). Same-sheet formulas come through. ODS formulas are rewritten into the page's notation (`[.A1:.A3]`
becomes `A1:A3`). calamine reads XLSB's shared formulas wrongly and garbles its references (`$BTRP$2` for `$D$2`), so
every XLSB cell is the value Excel stored, dates included, and a copy saved as XLSX has values where the formulas were.
FODS is the XML of an ODS's `content.xml`, so calamine's ODS reader reads it inside a package made in memory.
calamine gives dates but no other number formats, so thousands separators and percentages of XLS and ODS show as plain
numbers.

**DBF.** The field names are the header row. Text is in the code page the header names, or, when it names none, guessed
as a CSV's is (which is how GBK files from old Chinese systems come out right). Dates, numbers, logical fields and Visual
FoxPro's integers, currency, doubles and date-times are read; memo fields come from the `.fpt` or `.dbt` beside the
table; deleted records are left out.

**SQLite.** A sheet per table, then per view that reads within 5 seconds; the column names are the header row. The
database is opened `immutable`, so nothing is written or locked; when a `-wal` or `-journal` file is beside it, the
database and that file are copied to a temporary folder and the copy is read. Triggers are off, the schema is not
trusted, extensions cannot be loaded, and there is no SQL from the person. The whole file is never read into memory: each
table shows at most 100,000 rows and the file at most 2,000,000 cells (each table at least 1,000 rows); a table that was
cut says so in the Info panel. A blob that is not text shows as hex (`0x89504E47…`). An encrypted database (SQLCipher)
has no SQLite header and opens as damaged.

**Large tables.** Only what is on screen is drawn, so a hundred thousand rows feel like ten.

**Not done.** Editing charts, pivot tables, conditional formats or merged cells; writing XLS, XLSB, ODS, FODS, DBF or
SQLite; SQL; whole-column references (`A:A`) do not move when columns are inserted; array formulas are kept but not
worked out; column widths dragged in the window are written only into a new package, not into a patched one; charts and
pivot tables of XLS and ODS are not carried into a saved XLSX.

## Windows and the Mac

- **Windows.** Each workbook opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button.
- **The Mac.** One program shows each workbook in a window of its own, and each window has its own file
  (`dotany_base::builder_per_window`). `Info.plist` and `lproj/` name the document types Sheets opens and the languages
  of its name, and are written by `shared/scripts/mac-plist.mjs`.
