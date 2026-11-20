# dotany-convert

The converter of the DotAny hub, as a Rust library. Files go in; the same thing comes out in another format, saved
beside each original under a name that is free, and the original is never opened for writing. The hub's page and its
right-click window ("Convert with DotAny") call it; it has no window of its own.

Each family of files is read and written by the same code the matching program uses, without the window around it. Most
of that code is not a copy: the program and the converter build from one crate, so a fix reaches both.

| Family | Crate shared with the program |
|---|---|
| Pictures (DotAny Image) | `dotany-picture`, `shared/picture` |
| Sound (DotAny Audio) | `dotany-sound`, `shared/sound`; with `dotany-sound-encode`, `shared/sound-encode`, for MP3 and FLAC |
| Tables (DotAny Sheets) | `dotany-table`, `shared/table` |
| Words (DotAny Text) | `dotany-words`, `shared/words` |
| Archives (DotAny Archive) | `dotany-unpack`, `shared/unpack` |

Where a program does a format only in its page (three.js, PDF.js, Slides' drawing), the converter either does the same
work in Rust or leaves the format out. The table below says which.

## What it reads and writes

"Written" means the converter makes it. A format that is only read can be a source but never a target.

| From | To | Notes |
|---|---|---|
| png, jpg, webp, tiff, bmp | png, jpg, webp, tiff, bmp | Upright, as Image shows it. The colour profile, EXIF and XMP are carried across. JPG has no transparency: what is see-through becomes white ("Transparent areas will be filled with white."). |
| gif, ico, tga, qoi, pbm, pgm, ppm, pnm, pam, farbfeld, pcx, iff, ilbm, lbm | png, jpg, webp, tiff, bmp | Read only. A moving GIF or WebP, or a TIFF of many pages, converts its first frame or page. An ICO converts its largest size. HEIC, AVIF, camera RAW and arithmetic-coded JPEG are refused whatever the file is called. |
| mp3, flac, wav, aiff, ogg | mp3, flac, wav, aiff, ogg | MP3 by LAME at 128, 192 or 320 kbps (Standard / High / Maximum), FLAC by libFLAC, Vorbis at 192 kbps, WAV and AIFF as PCM in the source's depth (16 or 24 bit). Title, artist, album and cover are carried. Sound is streamed, so a long recording never sits in memory. MP3 above 48 kHz goes down to 48 kHz; more channels than the target holds are folded. M4A, Opus, AAC and WMA are refused. |
| mod, xm, s3m, it, au, snd, voc, WAV in A-law or μ-law | mp3, flac, wav, aiff, ogg | Read only. Tracker modules are rendered once through by libopenmpt at 48 kHz stereo, up to 30 minutes, with title and artist carried. Other tracker formats, ADPCM and G.72x are refused. |
| xlsx, xlsm, xls, xlsb, ods, fods, dbf, sqlite | csv, tsv | The first worksheet's values ("Exports the first sheet only, without formatting."); dates are written as `2026-01-05`; UTF-8 with the byte-order mark Excel looks for. |
| xls, xlsb, ods, fods | xlsx | Every sheet, values and formulas, dates as dates. No fonts, colours, widths or merged cells. |
| dbf | xlsx | Field names as the first row; text in the code page the file names (or guessed, as for a CSV); memo fields from the `.fpt` or `.dbt` beside it; deleted records left out. |
| sqlite, sqlite3, db | xlsx | A sheet per table, then one per view that reads within 5 seconds; column names as the first row. The database is read in place and never written. A table of more than 1,048,575 rows, or a database of more than 5,000,000 cells, is refused as too large ("“{name}” is too large to convert.") instead of being converted in part. `.db` is converted, but the hub offers it no right-click entry, because many unrelated programs use that extension. |
| xlsm | xlsx | The whole workbook as it was, byte for byte, without its macros. |
| csv, tsv | xlsx, csv, tsv | A typed number is a number; `007` and long IDs stay text. CSV to TSV and back keeps the source's encoding (GBK stays GBK). |
| pdf | png | One picture per page, drawn by the system's own PDF renderer on white at twice the page's size in points ("Exports each page as an individual image."). |
| docx, doc, rtf, odt | docx, md, txt | doc, rtf and odt to docx give the Word document Text shows. To Markdown: headings, lists, tables, bold and italic are kept; colours, sizes and spacing are dropped. The pictures are written beside the Markdown as `<name> 图1.png`… in their own format, linked by relative paths; they are not among the outputs `convert` returns. To text: the words, a line per paragraph, table cells separated by tabs. |
| md | docx, txt | A file shown as an article keeps its headings, lists, tables and pictures; a file that holds code blocks, quotes or HTML goes to Word a line per paragraph, its words unchanged. |
| txt | docx, md | To docx a paragraph per line; to md the same words with the extension changed. |
| glb, gltf, obj, stl, 3mf, ply, off | glb, obj, stl | Each format keeps its own unit and up axis (glTF Y up in metres, STL Z up in millimetres, OBJ Y up in centimetres, 3MF Z up in the unit it names). STL holds triangles only ("Preserves 3D geometry only, excluding colors and animations."). OBJ holds shapes, normals and texture coordinates, no materials. GLB carries colours and pictures. A gltf becomes a glb by packing its buffers and pictures into one file without decoding them, so animations, skins and Draco or Meshopt shapes survive. A PLY of points alone has no shape to write and is refused. |
| zip, 7z, tar, tar.gz, tar.bz2, tar.xz, tar.zst, rar, lzh, cpio, ar, deb, rpm, gz, bz2, xz, zst | zip, 7z, tar | Every entry, folders and times included, in one pass. The compression levels are Fast / Balanced / Ultra. RAR, LHA, cpio and ar are read by libarchive's readers and never written. Names an old zip keeps in a local code page (GBK, Shift-JIS) are decoded with one `chardetng` guess over all of them. An encrypted entry or name stops the file with `Locked`. |

Left out on purpose; `targets` answers `Unsupported` for them:

- pptx, ppsx, potx: a deck is drawn only by Slides' page.
- PDF to Word: only a PDF viewer's page reading of the text and layout makes it.
- vox, gcode, gco, nc, and fbx: no reader for them is built in.
- A glb or gltf with Draco or Meshopt shapes cannot go to obj or stl, because only 3D's page unpacks those shapes.

Video (mp4, mov, mkv and the rest of the list in `src/lib.rs`) is never converted: `targets` answers `Video`.

## Using it

Add it as a path dependency (this is the line in `hub/src-tauri/Cargo.toml`):

```toml
dotany-convert = { path = "../../shared/convert" }
```

```rust
pub fn targets(paths: &[PathBuf]) -> Result<Targets, Refusal>
pub struct Targets { pub family: Family, pub formats: Vec<String> }      // formats in the order the hub shows them
pub enum Refusal { Video, Mixed, Unsupported }

pub fn estimate(paths: &[PathBuf], target: &str, opts: &Options) -> u64  // bytes, all outputs together
pub struct Options { pub quality: u8, pub tone: Tone, pub pack: Pack, pub picture: String }
    // quality 10–100 (jpg, webp); tone Normal|High|Top (mp3); pack Fast|Normal|Small (zip, 7z);
    // picture "图{n}" (Word to Markdown: what the pictures written beside the file are called)

pub fn convert(paths: &[PathBuf], target: &str, opts: &Options, progress: impl Fn(f32))
    -> Result<Vec<PathBuf>, ConvertError>
pub struct ConvertError { pub reason: Reason, pub file: PathBuf, pub done: Vec<PathBuf> }
pub enum Reason { Refused(Refusal), Damaged, Unreadable, Locked, Unwritable, TooLarge, Missing(String) }

pub fn notices() -> &'static [Notice]                                    // every library carried, with its licence
```

- `Family` serialises as lower-case names: `picture`, `sound`, `table`, `page`, `pdf`, `words`, `model`, `pack`.
  `Options`, `Tone` and `Pack` read the same words, for example `{"quality":85,"tone":"high","pack":"normal"}`.
- **What is offered.** All files must belong to one family, and a format is offered when at least one file is not
  already in it. A file already in the chosen format is left alone. Films give `Video`; files of two families give
  `Mixed`; anything else the build cannot take gives `Unsupported`. Ask `targets` rather than keeping a table of your
  own: the converter reads more than the programs list in their dialogs.
- **Names.** `photo.png` becomes `photo.jpg`, or `photo (2).jpg`, `photo (3).jpg` if those exist. A PDF's pages are
  `name 1.png`, `name 2.png`… (one page: `name.png`).
- **Stopping.** On a damaged, locked or unreadable file the conversion stops there: that file's part files are removed,
  outputs already finished stay and are listed in `done`, and later files are not started. Nothing asks for a password;
  a file that needs one is `Locked`.
- **Progress** runs from 0 to 1 over all files. Each file's share follows its size, and it moves within a file.
- **Estimates** are quick enough to ask on every move of a slider: pictures encode a sample of about a quarter of a
  million pixels of the real picture (the last picture read is kept), sound works from the header's length, a PDF draws
  its first page, and tables, words and archives apply ratios.
- `convert` blocks; call it from a worker thread. PDF pages are drawn on a thread of their own, so the caller's COM
  apartment does not matter.
- **Ship LAME beside the program** (`libmp3lame.dll`, `libmp3lame.dylib` on the Mac). `dotany-sound-encode`'s build
  places it in the target folder. Without it MP3 stops with `Missing("libmp3lame.dll")`.
- **Licence page.** The Rust crates are found by `shared/scripts/notices.mjs` by themselves. The licence texts of the C
  libraries it cannot see come from `notices.mjs` in this folder: pass `convertNotices()` as `before` when generating a
  program's notices.

## Building

It is a library: `cargo check` and `cargo build` in this folder work with Rust stable (1.80 or later) on Windows and
macOS. It builds no C code of its own; LAME, libFLAC, libopenmpt and libarchive are compiled by the crates that carry
their sources (`shared/sound-encode`, `shared/sound`, `shared/unpack`), and libwebp comes with `libwebp-sys`. The release
profile is size-optimised (`opt-level = "s"`, LTO, one code-generation unit).

## Where things are

- `src/lib.rs`: the interface, the table of which extension belongs to which family and what each family can be written
  as.
- `src/out.rs`: names, part files and the final move.
- `src/picture/`: pictures, on `dotany-picture`; `encode.rs` writes the five formats.
- `src/sound/`: sound, on `dotany-sound`; each decoded block goes straight to the writer.
- `src/table/`: tables, on `dotany-table`; `fresh_xlsx` writes a new workbook.
- `src/words/`: words, on `dotany-words`; `md.rs` holds the Markdown rules in Rust.
- `src/pack/`: archives, read through `dotany-unpack`; `write.rs` writes zip, 7z and tar.
- `src/model.rs`: 3D models, as glTF, OBJ, STL, 3MF, PLY and OFF.
- `src/pdf.rs`: PDF pages drawn by the system.
- `src/notices.rs` and `notices.mjs`: the list of what the converter carries, and the licence texts of the C libraries.

Two modules are copies of a program's file rather than shared code: `src/picture/encode.rs` (Image's encoder without
the moving GIF and WebP writers) and `src/pack/write.rs` (Archive's archive writer, which has since gained one thing
this copy lacks: names composed for the Mac, which a conversion has no use for, as its names come from the archive it
reads and not from a disk). A fix in either belongs in both
places.

## How it works

`targets` finds each file's family from its extension and offers the family's formats. `convert` then takes the files
one by one. For each it opens an `Outputs` sink, which hands out names beside the original; the family's module reads
the source, writes into the sink, and the sink is committed or abandoned. A file is judged by its first bytes, not by its
name, where the format allows: a file named `.xlsx` that is something else is `Damaged`.

## What it is careful about

- **Never over an original.** Each output is written to a hidden part file beside the source and then moved to its real
  name by a move that fails rather than replaces (`MoveFileExW` without the replace flag on Windows, `renamex_np` with
  `RENAME_EXCL` on the Mac, after the bytes are flushed to disk). Nothing is overwritten even if another program takes
  the name in between.
- **Nothing half-made is left.** On any error the part files of that file are removed; dropping a sink without
  committing it removes them too.
- **Memory.** Sound, archives and databases are streamed or read a table at a time. Pictures and Word documents are held
  whole; a source too large for the memory there is gives `TooLarge`.
- **Format limits.** A WAV past 4 GB, a spreadsheet past its row limit and a database past the cell limit are refused
  rather than converted in part.
- **Refused formats stay refused** whatever the file is renamed to: files are identified by their bytes.
- **Placeholder words.** A Word paragraph the converter cannot show (a drawing without text) is written into Markdown
  and text as a placeholder in the language of the interface that asked (`Options::lang`, a code of one of the nine
  languages; English when unknown). The sentences are DotAny Text's own for its picture and "unsupported elements"
  placeholders (`src/words/locked.rs`, checked against `text/src/lang/*.json` by a unit test).

## Windows and the Mac

Both are built. The differences are small: PDF pages are drawn by `Windows.Data.Pdf` on Windows and by Core Graphics
(`CGPDFDocument`) on the Mac, so a page can differ a little from what DotAny PDF's own viewer draws (fonts, form
fields); the final move is the platform's own, described above; and the MP3 library is `libmp3lame.dll` or
`libmp3lame.dylib`. On any other system `targets` answers `Unsupported` for PDFs, as no renderer is available.
