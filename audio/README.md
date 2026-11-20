# DotAny Audio

DotAny Audio opens a song, shows its wave, lets you cut it and adjust it (volume, evening out, fades, reverb) and edit
its tags and cover, and saves it as MP3, FLAC, WAV, AIFF or Ogg Vorbis. It leaves the file alone until you save. It is a
[Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with esbuild) and a Rust program in
`src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon.

## What it opens and saves

A file is what its first bytes say, not what its name says.

| Format | Opens | Can be saved as |
|---|---|---|
| MP3 (Layer III) | yes | MP3, FLAC, WAV, AIFF, Vorbis |
| FLAC | yes | the same five |
| WAV: 8, 16, 24 and 32-bit integer, 32 and 64-bit float, A-law, μ-law | yes | the same five |
| AIFF, and AIFC holding plain samples | yes | the same five |
| Ogg Vorbis (`.ogg`, `.oga`) | yes | the same five |
| M4A: AAC, HE-AAC, ALAC; an MP4 file whose only tracks are such sound | yes, read by the system's decoder | a new file in one of the five |
| Tracker modules: MOD, XM, S3M, IT | yes, played through once, read only | a new file in one of the five |
| AU (`.au`, `.snd`) and VOC: plain samples and G.711 | yes, read only | a new file in one of the five |

Raw AAC (ADTS), Opus (also in Ogg or MP4, also renamed), WMA, MP1, MP2, AC-3, DTS, ADPCM, FLAC in MP4, an MP4 with
a picture track and anything else that is not in the table are refused with a notice, even when they are named like a
song. No code for them is compiled in: `scripts/check-deps.mjs` runs on every build and fails it if a crate, a Symphonia
feature or a vendored library that reads them comes in.

## What you can do

- See the whole wave or zoom into it, select a stretch, keep only the selection or delete it, and undo and redo up to 60
  steps.
- Play and pause, jump 5 seconds with the arrow keys, and step to the previous and next song in the folder.
- Adjust the song: volume (±12 dB), evening out to a common loudness, fade in and fade out (up to 10 seconds each),
  and reverb (room or hall, with a strength). Playing and saving use the same arithmetic.
- Edit the title, artist, album and cover (JPEG or PNG).
- Choose the format and, for MP3 and Vorbis, the quality. The save card says what the save will do to the sound and how
  large the file will be before anything is written.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This checks the dependencies (`node scripts/check-deps.mjs`, which can also be run alone), builds the page
(`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-audio.exe` in `release/` under Cargo's
target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). On Windows `libmp3lame.dll` is built into the
same folder; both files belong together (see MP3 and LAME below). The page build is the family's,
`shared/scripts/build.mjs`; it also writes `src/icons.js` from `lucide-static`. `src/notices.generated.js`, the licence
notices the program shows, is written by `shared/scripts/notices.mjs` from what is really built in. The Mac's
application and its `Info.plist` and `lproj/` folders are made by `shared/scripts/mac-release.mjs` and
`shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Audio itself (`TOOL`), the song open in each window and the commands the page calls.
- `decode.rs`: from a file's bytes to samples. `store.rs`: the samples, in memory or in a temporary file, with the peaks
  and loudness measured while they are written.
- `fx.rs`: the recipe and the one chain of arithmetic that carries it out: spans, fades, volume, reverb. `engine.rs`:
  playing. `export.rs`: what a save does and how large it will be.
- `platform/`: what is Audio's own to ask of the system, the same function on each: decoding M4A. `media_foundation.rs`
  is Windows' (Media Foundation), `macos.rs` the Mac's (AudioToolbox).

**Shared with the rest of DotAny:**

- `dotany-sound` (`../shared/sound`), which the hub's converter uses too: `formats.rs` (what a file is, by its bytes,
  and what is refused), `oldsound.rs` (AU and VOC), `module.rs` (tracker modules), `encode.rs` (each format's bytes),
  `rate.rs` (sample rates) and `tags.rs` (words and cover). `src-tauri/src/lib.rs` hands them on as `crate::formats`
  and so on.
- `dotany-sound-encode` (`../shared/sound-encode`), which Recorder uses too: `flac.rs` and `lame.rs`, the doors to those
  libraries. The C sources of LAME's encoder and libFLAC are in its `vendor/`, and libopenmpt's in `shared/sound/vendor/`.
- `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files, the watcher that notices when
  another program changes the file, fonts and the platform layer.

**The page** (`src/`):

- `main.js`: the wave, editing it, the dock, the adjust and info panels, and the save card's options. `player.js`: the
  page's side of playing. `backend.js`: the calls to the Rust side. `i18n.js` and `lang/`: Audio's words in each
  language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings, the Save As card, fonts, keys,
  trackpad gestures, sound, and the family's words and licences. `shared/web/family.css` comes first in the built
  stylesheet; `src/styles.css` only adds to it.

## How it works

**Samples live in Rust and never change.** A file is decoded once, whole, to 32-bit floats. Up to 512 MB stay in
memory; beyond that they go to a temporary file under the data folder, read back through a memory map and deleted with
the song (and swept at the next start if the program was killed). The disk file is not needed again except to copy it.

**The page holds a recipe:** which spans of the source remain, volume, evening out, fades and reverb. Cutting makes a
new list of spans, so it is instant at any length and undo is keeping the old list. "Unsaved" means the recipe differs
from what is on disk, so undoing back to it makes the file clean again.

**Preview and export are one piece of code.** `fx.rs` runs for the speakers and for every save. Rust computes the
wave's peaks from summaries kept at three scales (a peak every 256, 4096 and 65536 frames); gain and fades are
multiplied onto them in the page, so a slider never asks Rust anything for the picture.

**Playing.** A thread reads the store, runs the chain, changes the sample rate to the device's and keeps 60 ms waiting
in a ring. The system's audio callback (through cpal) only takes from the ring. A jump ramps over 128 samples so it does
not click. The device is closed after 3 seconds of quiet, so the next song finds whatever is the default then; a device
lost mid-song is replaced by the new default, and playing goes on from where it stopped.

**Cuts** fade each inner edge over 2.5 ms, so a join does not click. Everything further than that from a join is
sample-exact.

**Reverb** is decaying noise, 0.7 s long for the room and 2.8 s for the hall, scaled so that an impulse of any length is
about as loud as any other, and mixed in at a level the strength slider sets. A stereo file gets a different impulse on
each channel; a mono file gets one and stays mono. Dry plus reverb can pass full scale, so with reverb on, a limiter
holds the output at 0.98 and comes back over about 100 ms. The noise is seeded, so a save is the same every time. A save
includes the ring-out, so the file is longer than the song by the reverb's length: that is what is heard when it plays.

**Evening out** measures loudness as ITU-R BS.1770 does, with its gating, and aims at -16 LUFS without letting any
sample pass 0.98 of full scale. The energy of every 100 ms is kept while decoding, so the answer is available at once
after a cut.

## What it is careful about

**Formats, by the bytes.** `shared/sound/src/formats.rs` looks at the start of a file (and, for MP4, walks its boxes to
the movie box, which may be at either end) and says what it is, so a refused file is called unsupported rather than
damaged. Symphonia reads MP3, FLAC, WAV, AIFF and Ogg Vorbis, with only `mp3 flac ogg vorbis pcm wav aiff` switched on;
AU, VOC and G.711 WAV are read by `oldsound.rs`, tracker modules by libopenmpt built with none of its codec switches,
and M4A by the system.

**M4A is read by the system, never written** (`platform/`). An MP4 whose only tracks are sound in AAC or ALAC is M4A,
whatever it is called. The system decoder hands over 32-bit floats into the same store as every other format, so
playing, the wave, cutting and every adjustment work as for any song. On Windows the decoder also hands over the
encoder's priming and padding, so the file's edit list is read and they are dropped; on the Mac the reader drops them
itself. ALAC keeps its bits and counts as lossless: saving it as FLAC does not say the sound will not improve. Media
Foundation is looked up at run time, not linked, so a Windows without it (the N editions) still starts and says what to
add. Saving offers the five formats, starts on MP3, says the original stays, and refuses in Rust to write M4A or to
write over the M4A. Tags carry over into the new file.

**Saving** has three routes (`export.rs`):

| Route | When | What happens |
|---|---|---|
| Copy | same format, same sound, same words, no quality chosen | the same bytes; saved over itself, nothing happens at all |
| Tags | same format, same sound, new words or cover | the file is copied to the temporary file and only its tags change; no byte of the sound moves |
| Encode | anything else | from the stored samples, so a later save does not stack losses on the last one |

Each writes a temporary file in the target's folder, flushes it, then swaps it in. A failure at any point leaves what
was there, and the edits stay in the window. The system's save dialog asks before anything is replaced.

**Quality.** The chips are 128, 192 and 320 kbps for MP3 and 112, 192 and 320 for Vorbis. With nothing changed no chip
is lit: nothing is compressed unless someone picks a quality. Once the sound or the format changes, 192 starts lit. A quiet line under the format buttons says what will
happen and what the save loses.

**Bits and rates.** 16-bit stays 16 and 24-bit stays 24, bit for bit, in FLAC, WAV and AIFF. 8-bit becomes 16. Deeper
and floating-point sources become 24, and the line says so. Lossy sources are written as 16. Nothing is dithered.
MP3 cannot hold a rate above 48 kHz or more than two channels: those are converted, and the line says so. WAV and AIFF
count their size in 32 bits, so a save that would pass 4 GB is refused before anything is written.

**Tags** go through lofty, as ID3v2.3 where ID3 is used. A re-encoded file carries everything the source's main tag
held. A title that was only ever the file's name is not written into the file unless someone edits it. An old ID3v1
tag is updated, or removed if it cannot hold the new words.

**After a save the window is on the file it wrote,** in any format; its samples stay the original's. **Another program
changing the file** puts up the notice row; "Keep" marks the file as no longer the one the sound came from, so saves
encode rather than copy from it. A copy also checks the file's size and time first.

**MP3 and LAME.** LAME is LGPL. It is built from `shared/sound-encode/vendor/lame-3.100` into its own library,
`libmp3lame.dll` on Windows and `libmp3lame.dylib` on the Mac (in the application's `Contents/Frameworks`), which sits
beside the program and is opened when an MP3 is first saved. Anyone can replace that file with their own build. LAME's
decoder (`mpglib`, `mpglib_interface.c`) is not in the tree and `HAVE_MPGLIB` is never defined; `check-deps.mjs` fails
the build if either changes. The licence page names LAME, prints its licence, says where the source is and how to swap
the library. libFLAC (BSD) is compiled into the program from `shared/sound-encode/vendor/flac`. Vorbis comes through
`vorbis_rs` (BSD), and tracker modules through libopenmpt (BSD, built from `shared/sound/vendor/libopenmpt` without the
decoders its release carries).

## Windows and the Mac

- **Windows.** Each song opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons. `windows-app-manifest.xml` asks for version 6 of the common controls, which the
  cover's open dialog uses, and declares Windows 10 and 11 support, which Windows requires before it shows the snap
  layouts over the maximise button. M4A is decoded by Media Foundation. Sound goes out through WASAPI.
- **The Mac.** One program shows each song in a window of its own, and each window plays on its own, as two QuickTime
  windows do. M4A is decoded by AudioToolbox's `ExtAudioFile`, which always has the decoders, so a failed M4A there is
  a damaged file, never a missing component. Two fingers on a trackpad move a zoomed wave and a pinch zooms the time
  about the fingers. `Info.plist` lists Ogg, tracker modules, AU and VOC as the types Audio is the default for, and
  MP3, FLAC, WAV, AIFF and M4A as the types it also opens. Sound goes out through Core Audio.
