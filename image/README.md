# DotAny Image

DotAny Image opens a picture, lets you look at it, make the common changes and save it, and leaves the file alone until
you save. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with esbuild) and a
Rust program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon.

## What it opens and saves

| Format | Opens | Can be saved as |
|---|---|---|
| JPG, PNG, WebP | yes | JPG, PNG, WebP |
| BMP, TIFF (every page) | yes | the same format, PNG, JPG, WebP |
| GIF, animated WebP | yes, and they play | GIF, WebP, PNG |
| ICO (every size) | yes | ICO, PNG |
| SVG | yes | SVG (an exact copy), PNG |
| TGA, QOI, PBM, PGM, PPM, PAM, farbfeld, PCX, IFF ILBM | yes, read only | PNG, JPG, WebP |

A file is what its first bytes say, not what its name says. HEIC, AVIF, PSD, JPEG XL, JPEG 2000, JPEG XR and camera
RAW files are refused with a notice, even when renamed to `.jpg`. No code for those formats is compiled in:
`scripts/check-deps.mjs` runs on every build and fails it if a crate or a feature of the `image` crate that reads them
comes in.

## What you can do with a picture

Zoom and move around it, step to the previous and next picture in its folder, turn and flip it, crop it (free or to a
ratio), adjust exposure, contrast, saturation, temperature, highlights and shadows (or pick a preset), draw with a
brush, add text, undo and redo, resize it when saving, remove its photo details, and print it. A moving picture can be
turned, flipped and cropped; an SVG is shown, printed and saved, and its source opens in DotAny Text when Text is
installed. A picture can also be pasted from the clipboard.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-image.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the family's, `shared/scripts/build.mjs`; it also writes `src/icons.js` from
`lucide-static`. `src/notices.generated.js`, the licence notices the program shows, is written by
`shared/scripts/notices.mjs`. The Mac's application and its `Info.plist` and `lproj/` folders are made by
`shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Image itself (`TOOL`), the picture open in each window, and the commands the page calls.
- `decode.rs`: from a file's bytes to upright 8-bit RGBA, one slot per TIFF page or ICO size, moving pictures frame by
  frame. `store.rs`: the frames of a moving picture, in memory and in a temporary file.
- `export.rs`: what a save will do, its estimated size, and doing it. `encode.rs`: the bytes of each format written.
- `colour.rs`: colour profiles. `svg.rs`: the SVG sanitiser. `folder.rs`: the pictures beside this one. `memory.rs`: how
  much memory a large picture may still take.

**Shared with the rest of DotAny:**

- `dotany-picture` (`../shared/picture`), which the hub's converter uses too: `formats.rs` (what a file is, what is
  refused), `ops.rs` (turning, cropping, the sliders, drawing, resizing), `metadata.rs` and `exif.rs` (everything that
  is not pixels), and `pcx.rs` and `ilbm.rs`, readers for PCX and IFF ILBM.
- `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files, the watcher that notices when
  another program changes the file, fonts and the platform layer. Image answers `can_open` itself, so that a camera
  RAW or HEIC file named like a picture is called unsupported rather than damaged, and tells the watcher to ignore a
  file whose time changed but whose bytes did not.

**The page** (`src/`):

- `main.js`: the window, the picture and the tools. `render.js`: drawing the picture through the sliders, on the
  graphics card or, without one, on the processor. `backend.js`: the calls to the Rust side. `i18n.js` and `lang/`:
  Image's words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, fonts, the
  text caret, keys, trackpad gestures, sound, and the family's words and licences. `shared/web/family.css` comes first in
  the built stylesheet; `src/styles.css` only adds to it.

## How it works

**The pixels live in Rust and never change.** For each slot the page holds a recipe: which way round, the crop, the
sliders and what is drawn. Undo walks back through the recipe, so it costs no pixels. Turning is done when the picture
is drawn on screen, so it is instant at any size. A save runs the recipe over the full pixels in a fixed order: turn,
crop, sliders, drawing, size. The preview uses the same order.

**Previews.** With a graphics card, a picture up to 16 megapixels (and 8192 pixels on its long side) reaches the page
pixel for pixel, and a larger one as a copy at most 4096 pixels on its long side; without one, the copy is at most
2048 pixels. Zoomed in past what that copy
holds, the page asks Rust for the part on screen, sharp, through the same arithmetic as a save. When a picture opens, a
copy at most 2048 pixels on its long side comes first so the window fills quickly. Pixels reach the page from their
own address (`dotpix:`), not through the command channel, so a large picture does not hold up the window.

**What is drawn** is rasterised in the page, because text needs the browser's fonts and shaping. On saving, the page
draws it again at full size in pieces of at most 4096 pixels, with the same rasteriser that draws it on screen, and
Rust lays the pieces over the pixels.

**The preview matches the saved file.** The sliders are the same formula three times: GLSL and JavaScript in
`render.js`, and Rust in `shared/picture/src/ops.rs`.

## What it is careful about

**Colour profiles.** A picture's numbers are not changed behind your back.

- Editing works on the file's own numbers, and its RGB colour profile is written back into JPG, PNG, WebP and TIFF, so
  a wide-gamut photo stays wide-gamut. Removing photo details keeps the profile: it describes the colours and names
  nobody.
- The screen shows the numbers converted to sRGB (a 33×33×33 table on the graphics card, made with `qcms`), so a
  wide-gamut picture does not look washed out. The monitor's own profile is not applied.
- GIF, BMP and ICO cannot say which colours their numbers mean, so a save to those converts to sRGB, and the Save As
  card says so. Colours outside sRGB are clipped in those formats only.
- The sliders work on the file's numbers, so the same slider acts a little differently on a wide-gamut picture than on
  an sRGB one.
- Brush and text colours are picked in sRGB and converted to the file's profile, so the colour picked is the colour
  seen.
- A grey or CMYK profile describes numbers that no longer exist after decoding to RGB, so it is not carried. CMYK
  pictures become RGB. A PNG's `gAMA` and `cHRM` chunks without a profile are ignored.

**A turned JPG is not recompressed.** Turning or flipping a JPG, and nothing else, changes only its EXIF orientation:
every byte of the picture data stays the same, and all eight orientations are possible. Rotating the compressed blocks
instead would trim the edges of a picture whose size is not a multiple of 8 or 16. A program that ignores the
orientation shows the old direction; current browsers, phones and Windows Photos honour it. Moving the quality slider,
cropping, adjusting, drawing or resizing re-encodes, so a JPG crop is not lossless.

**Photo details** (EXIF, XMP and the like) are kept unless you remove them. When the pixels really change, the details
are edited in place: the orientation set to upright, the sizes updated, and the small preview inside erased, since it
would still show what was cropped away. Removing them leaves only the colour profile (and, on a turned JPG, a block
holding nothing but the orientation), also drops anything stored after the end of a JPG, such as a phone's motion
clip, and the finished bytes are checked before the file is written: if anything is left, the save fails. Saving to
TIFF drops the camera maker's notes, whose insides point at places in the old block. BMP, GIF and ICO cannot carry
details, and the Save As card says so.

**Saving.** The file is written to a temporary file in the same folder, flushed, then swapped in, so a failed save
leaves the old file whole; the edits stay and the window says why. The system's save dialog asks before replacing a
file. A save with nothing edited and the quality slider untouched is a byte-for-byte copy, so 16-bit pictures and
everything else in the file survive it. Saving the whole picture in its own format moves the window to the new file.
Saving in another format writes a new file and leaves the window on the original, still unsaved.

**8 bits per channel.** Editing is 8 bits per channel. Re-encoding a deeper picture loses the extra depth, and the Save
As card says so.

**SVG.** An SVG can hold scripts, so the page only ever gets a copy rebuilt from the parts that draw: no scripts, event
handlers, DOCTYPE, foreign objects or embedded documents, and no links except to places inside the file and to
embedded raster images. It is shown as an image, which itself forbids scripts and requests, under a content security
policy that forbids them again. The file on disk is never touched, and saving it as SVG copies it as it was.

**Moving pictures.** Frames are packed one by one: the first 256 MB stay in memory, the rest go to a temporary file
deleted with the picture. Reading continues in the background while the picture plays. The page keeps at most 128 MB
of frames, at no more than 1600 pixels on the long side, and asks for the rest as it plays. The file's own bytes stay
in memory for an exact copy on saving. An animated PNG shows its first frame, and a save that re-encodes it says that
only one frame is kept.

**Memory.** A picture that would need more memory than is free is refused with its own message instead of slowing the
computer to a halt.

## Windows and the Mac

- **Windows.** Each picture opens in a program of its own, with one window. The window has no system title bar: the
  page draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button. Free memory is read with
  `GlobalMemoryStatusEx`.
- **The Mac.** One program shows each picture in a window of its own. On a Retina screen the sharp part of a zoomed
  picture is fetched for the screen's own pixels. Two fingers on a trackpad move a picture larger than the window, a
  pinch zooms, turning two fingers turns the picture a quarter at a time, and smart zoom (a two-finger double tap)
  zooms in or back; a mouse wheel zooms. Free memory is the system's own figure
  (`kern.memorystatus_level`). Printing gives the sheet's height to the page, because WebKit's `vh` is the window's
  height when printing.
