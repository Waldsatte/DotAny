# DotAny Video

DotAny Video plays a film, cuts it without re-encoding, changes its title and cover, shows subtitles beside it, and
leaves the file alone until you save a new one. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain
JavaScript, built with esbuild) and a Rust program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and
on Macs with Apple silicon.

The program holds no decoder and no encoder. It plays through the system's player and rewrites only the boxes and
elements around the pictures and sound.

## What it opens and saves

| Format | Extensions | Played by | Cut, title and cover | Saved as |
|---|---|---|---|---|
| MP4, MOV | `.mp4` `.m4v` `.mov` | The system's player (WebView2, WKWebView) | yes | the same extension |
| Matroska | `.mkv` | Windows: the system's player. Mac: as it is when the picture is VP8 or VP9, otherwise repackaged | yes | the same extension |
| WebM | `.webm` | The system's player | yes | the same extension |
| AVI | `.avi` | Windows: Media Foundation. Mac: repackaged | no | an exact copy |
| WMV | `.wmv` | Windows: Media Foundation. Mac: not played | no | an exact copy |
| Subtitles | `.srt` `.vtt` `.ass` `.ssa` | Shown over the film, never written into it | | |

A file is what its first bytes say, not what its name says: a name that promises a film over bytes that are not one is
reported as damaged. A picture format built on MP4's boxes (HEIC, HEIF, AVIF) is not a film. A film keeps its own
format when saved, because nothing in it is re-encoded.

Whether a given film plays is up to the system: the page draws the first picture before it takes over the window, and
a film whose picture cannot be drawn is refused with a notice while the film already open carries on. A sound the
system cannot decode does not stop a film from opening; a second into playing, if no sound has come out, the window says
so.

## What you can do with a film

Play, pause, step five seconds with the arrow keys, change the volume (the up and down arrows) and the speed, go full
screen, and step to the previous and next film in the folder. Cut a part of it: the grips land only on keyframes or the
ends of the film, so the cut loses nothing, and playing stops where the cut ends. Give the film a title and a cover,
from a picture file or from the picture on screen. Load subtitles by choosing a file or dropping it on the window. Save
the result as a new file; the original is never changed.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which first runs `scripts/check-deps.mjs`, then writes `dist/`) and
then the program, `dotany-video.exe` in `release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise
`src-tauri/target`). The page build is the family's, `shared/scripts/build.mjs`; it also writes `src/icons.js` from
`lucide-static`. `src/notices.generated.js`, the licence notices the program shows, is written by
`shared/scripts/notices.mjs`. The Mac's application and its `Info.plist` and `lproj/` folders are made by
`shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

`scripts/check-deps.mjs` fails the build if a codec or container library (ffmpeg, libav, x264, x265, dav1d, vpx,
symphonia, mp4parse, a Matroska crate, and the like) is anywhere in the Rust dependency graph.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Video itself (`TOOL`), the film open in each window, and the commands the page calls.
- `formats.rs`: what a file is, from its name and its first bytes.
- `media.rs`: what the two containers share: the facts the page wants, the edit a save asks for, and a save as a list of
  pieces (`Plan`). `mp4.rs`: MP4, M4V and MOV. `mkv.rs`: MKV and WebM.
- `stream.rs`: one file served to `<video>` in stretches of at most 8 MB, from the program's own `dotfilm:` address.
- `platform/`: everything that differs by system. `media_foundation.rs` (Windows) plays AVI and WMV through Media
  Foundation and shares each picture with the page; `macos.rs` (the Mac) hands the player a repackaged film instead.

**Shared with the rest of DotAny:**

- `dotany-base` (`../shared/rust`): the window, atomic saves (`atomic`), settings, recent files, the watcher that notices
  when another program changes the open file, fonts, the files beside the open one, and the platform layer.
- `dotany-repack` (`../shared/repack`, the Mac only): an MKV or AVI as a QuickTime film, without decoding.

**The page** (`src/`):

- `main.js`: the window, the film, the dock with its timeline and trim, subtitles, the info panel and the save.
- `player.js`: the player around a `<video>`, with no tie to this window, so another tool can use it.
  `system-film.js`: the same face for a film Windows plays, drawn on a canvas. `subtitles.js`: reading subtitle files.
  `backend.js`: the calls to the Rust side. `i18n.js` and `lang/`: Video's words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, keys,
  trackpad gestures, sound and the family's words and licences. `shared/web/family.css` comes first in the built
  stylesheet; `src/styles.css` only adds to it.

## How it works

**Playing.** A `<video>` in the page plays the film from the program's own address (`dotfilm:`), which serves the
window's open film and nothing else, and answers each range request with at most 8 MB. Who decodes is therefore the
system: Microsoft's WebView2 Runtime on Windows, WebKit on the Mac.

**Opening.** Rust reads the container first (`Media::open`; a file whose insides cannot be read is still allowed to
play, with editing turned off). The film then plays in a `<video>` that is not yet on screen, and takes over the window
only once its first picture has been drawn, so a film the system cannot play is refused out loud and the film already
open carries on.

**A save is a plan first.** The new file is written down as a list of pieces (bytes the program made, stretches of the
source) before anything is written. So the size on the Save As card is exact, and a plan that cannot be carried out is
refused before it touches the disk. The write is atomic: a temporary file in the same folder, flushed, then swapped in.

**Cutting.** The list of keyframes comes from the file's sample tables (MP4), from its cues, or by walking its blocks
when it has none (MKV, WebM).

- MP4, M4V, MOV: new sample tables for what is kept, and an edit list saying which moment of each track the film starts
  on, so sound stays in step and pictures that only exist to complete a frame stay hidden. A film in fragments comes out
  as a plain file. Words and chapters in text tracks have their first and last samples shortened to the cut, and the
  QuickTime `chpl` chapter list is dropped. One sound sample before the cut is kept, silent behind the edit list, so the
  decoder has settled when the sound starts. Chunk offsets past 4 GB use `co64`, and `moov` comes first so the file
  plays while it loads.
- MKV, WebM: clusters are copied whole. Only the cluster holding the cut has blocks dropped and their two time bytes
  changed. The seek table and the cues are rebuilt, chapters move with the cut, and the muxer's statistics, which
  described the old file, are dropped.

A film whose edit lists do more than "wait, then play" cannot be cut, and says so; it can still take a new title and
cover. An encrypted film (a protected track in MP4, content encryption in Matroska) can be neither cut nor retagged.

**Title and cover.** MP4 family: `moov/udta/meta/ilst`, `©nam` and `covr`, the way Apple's tools and ffmpeg write them,
and the older QuickTime title is removed so the two cannot disagree. MKV: the Info title and an attachment named
`cover.jpg` or `cover.png`. The WebM profile of Matroska has no attachments, so a cover in a WebM file is written the same
way; players read it, a strict WebM validator would not accept it. The title field starts with the file's name when the
file has no title, and that is written only if someone edits it.

**Subtitles.** A subtitle file is read by the page: UTF-8, UTF-16 with a byte order mark, otherwise GBK. ASS and SSA
give only their words, in the family's subtitle style: the file's fonts, colours, places, effects and drawings are not
used, comments are not shown, and lines on screen at the same moment stand one under the other. Subtitles are never
written into the film, and tracks inside the film are not read.

## What it is careful about

**The file changing underneath.** A plan is only true of the file it was read from, so a save checks the size and time
of the source before it starts and again before the swap, and refuses rather than mix two files. Another program
changing the open file shows the family's row and stops playing.

**A failed save.** Where the new file goes is the system's save dialog's question, asked after the card's Save; that
dialog also asks before replacing a file. A failure leaves every file as it was, keeps the edits, and says why. After a
good save the window moves to the file just written, at the same place in the film.

**Nothing is decoded or encoded.** The pictures and sound of a saved film are the bytes of the original, copied
unread. Only the boxes (MP4) or elements (Matroska) that describe them are written new.

**Full screen** is the window itself going full screen. HTML full screen would only fill the page inside the window.

**Damaged files.** A length or count a file claims is checked against the file's real size before memory is set aside
for it, so a file that lies about itself is read as damaged, not followed.

## Windows and the Mac

- **Windows.** Each film opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button.
  - AVI and WMV are played by Windows, because WebView2 plays neither. Media Foundation's media engine reads the file
    and decodes it with the decoders Windows has, plays the sound and keeps the pictures in step. It runs in
    frame-server mode (no window, no graphics device) on a thread of its own: each new picture is copied, as BGRA, into
    memory shared with the page (WebView2's shared buffer, three pictures deep), and the page draws it on a canvas with
    a `VideoFrame`. `SystemFilm` in `system-film.js` has `RealFilm`'s face, so the window cannot tell them apart.
  - Media Foundation is looked up at run time rather than linked, so a Windows without it (the N editions) still starts,
    and such a film is refused with the same words as any film the system cannot play.
  - These films are only played: nothing in the program reads or rewrites AVI or ASF, so cutting is not offered, the
    info panel has no title or cover, and Save As is the file byte for byte. A picture shape stored only inside an AVI's
    video stream is not read by Windows, so such a film is shown with square pixels.
  - A film's picture and sound are decoded by whatever the system has: the WebView2 Runtime's own decoders and the
    codecs installed in Windows, so what plays differs from one Windows to another.
- **The Mac.** One program shows each film in a window of its own. WKWebView plays MP4, MOV and WebM by itself, and an
  MKV whose picture is VP8 or VP9. What it will not open is repackaged for it as a QuickTime film, without decoding
  (`dotany-repack`): an MKV with H.264, HEVC, MPEG-4 Part 2, MPEG-2 or Motion JPEG pictures, and an AVI with H.264,
  MPEG-4 Part 2, Motion JPEG or DV. The new film is read from the original as it plays: nothing is written to the disk.
  A film whose picture is none of those is refused, and a sound the Mac cannot play is left out.
  - WMV is not played; the page says so, not that a decoder is missing. A WebM or MKV with an AV1 picture is asked of
    WKWebView first, because its WebM player never answers about a picture it cannot decode; such a film is refused at
    once rather than after the 30 seconds the page waits for a first picture.
  - A cover or font stored ahead of the clusters, the usual place, makes WKWebView's WebM player hang. The page is
    served the file as it is with that element read as a Void element of the same length, so nothing after it moves and
    the file on disk is not touched.
  - After a seek while paused, WKWebView sometimes shows no picture for the new place; the page seeks once more, a
    millisecond on, which always brings it.
  - Two fingers sideways on the timeline move along it. The Mac's `⌘` and `⌃` keys belong to the system, so Video's
    letter keys (M for mute, F for full screen) do not act with them, and `⎋` leaves full screen through the system.
    In a window the top bar stays and the film is fitted below it; it steps aside only in full screen.
