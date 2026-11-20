# DotAny Recorder

DotAny Recorder records from the microphone, pauses, marks moments, and keeps what it hears on disk as it goes, so a
crash loses almost nothing. It plays a take back, saves a copy as WAV, FLAC or MP3, and hands a take to DotAny Audio for
editing. It is a [Tauri 2](https://tauri.app) program: a page in `src/` (plain JavaScript, built with esbuild) and a
Rust program in `src-tauri/`. It runs on Windows (on Microsoft Edge WebView2) and on Macs with Apple silicon.

## What it records, opens and saves

| Format | Records | Opens | Can be saved as |
|---|---|---|---|
| WAV, 16-bit PCM, one or two channels, 8 to 192 kHz | yes: one channel, at the microphone's rate | yes, and no other kind of WAV | WAV, FLAC, MP3 |

A file is what its first bytes say, not what its name says: a WAV of any other kind (8-bit, 24-bit, float) is refused
and left exactly as it is. A WAV from outside the recordings folder opens and plays like a take, but it is not in the
list and has no marks.

- WAV: 16 bits a sample, as recorded; refused above 4 GB, which is as much as the format can say.
- FLAC: 16 bits, lossless.
- MP3: 128, 192 or 320 kbit/s (the Save As card's Standard, High and Maximum), constant bit rate, at one of the MPEG rates, 8000
  to 48000 Hz. A take at any other rate is refused as MP3 before anything is written; the program asks a device set
  above 48 kHz for 48 kHz in the first place.

## What you can do

Choose the microphone (the system's default, or one by name), record, pause and resume, put a mark on the moment, and
stop. Every take is listed at the right, newest first; opening one draws its sound as a line you can click or drag to
move along, and plays it. Marks can be added while listening, too. Save As copies a take out as WAV, FLAC or MP3 and
renames the take in the list to the name it was saved under. A take can be opened in DotAny Audio from its row when
that program is installed beside this one, and deleted (one, or all of them, after the program asks). Space starts,
pauses, resumes and plays; `R` records or stops; `M` marks.

## Building

From this folder, with Node.js and Rust stable (on Windows, the MSVC toolchain):

```powershell
npm ci
npx tauri build --no-bundle
```

This builds the page (`node scripts/build.mjs`, which writes `dist/`) and then the program, `dotany-recorder.exe` in
`release/` under Cargo's target folder (`CARGO_TARGET_DIR` if set, otherwise `src-tauri/target`). The page build is the
family's, `shared/scripts/build.mjs`; it also writes `src/icons.js` from `lucide-static`, and adds the licence texts of
LAME and libFLAC, read from `shared/sound-encode/vendor`, to the notices the program shows. The build of
`dotany-sound-encode` compiles libFLAC into the program and leaves LAME beside it as `libmp3lame.dll`. Both files ship.
`src/notices.generated.js`, the notices themselves, is written by `shared/scripts/notices.mjs`. The Mac's application
and its `Info.plist` and `lproj/` folders are made by `shared/scripts/mac-release.mjs` and `shared/scripts/mac-plist.mjs`.

## Where things are

**The program** (`src-tauri/src/`):

- `app.rs`: what makes Recorder itself (`TOOL`), the take open in each window, the one recording in progress, and the
  commands the page calls.
- `capture.rs`: the microphone, the ring and the thread that writes the take as it is recorded. `takes.rs`: the
  recordings folder, a take's JSON, the list, recovery and deleting. `wav.rs`: the WAV header, reading, repairing and
  the peaks the page draws. `encode.rs`: a take written as WAV, FLAC or MP3. `stream.rs`: a file served to the page's
  `<audio>` in stretches of at most 8 MB. `mic_mac.rs` (the Mac only): the system's question about the microphone.

**Shared with the rest of DotAny:**

- `dotany-base` (`../shared/rust`): the window, atomic saves, settings, recent files, fonts, and the platform layer,
  including starting a sibling program (`sibling_tool`, used for DotAny Audio).
- `dotany-sound-encode` (`../shared/sound-encode`), which Audio uses too: `flac.rs` and `lame.rs`, the doors to libFLAC
  1.5.0 (compiled in) and LAME 3.100 (a separate library), with the C sources in its `vendor/`.

**The page** (`src/`):

- `main.js`: the window, the line of sound, the dock and the list of takes. `backend.js`: the calls to the Rust side.
  `lang/`: Recorder's words in each language, added to the family's.
- From `shared/web` (`@dotany/web/…`): the window's frame and top bar, menus, settings and the Save As card, keys, sound,
  and the family's words and licences. `shared/web/family.css` comes first in the built stylesheet; `src/styles.css`
  only adds to it.

## How it works

**A take is a file from its first moment.** One WAV per take in the recordings folder, `Music\DotAny Recorder` (with
`DOTANY_RECORDER_DATA` set, `<data>\takes`), 16-bit and mono, at the microphone's rate. Beside it is a JSON with the
take's name, when it began, its marks, and whether it is still being recorded. Both exist before the first sample does.
The channels of the microphone are averaged into one, so a voice is whole whichever side it came in on.

**Recording.** The microphone's callback pushes its samples into a ring (`rtrb`) with room for two seconds. A writing
thread empties the ring about every 10 ms into the file, straight through to the system, so what was heard is on disk
within a few hundredths of a second. Once a second it also syncs the file and rewrites its header with the length so
far. The same thread tells the page, about twenty times a second, the bars of the line (one bar for each fifteenth of a
second, the loudest sample in it), the level and how far along the take is.

**Listening.** The page's `<audio>` plays the take from the program's own `take:` address, which serves the take open in
the window that asked and nothing else, a stretch at a time. The line is drawn from the peaks Rust computes when the
take opens.

**Saving.** Save As reads the take in blocks and writes the new file in the chosen format through `atomic_produce`: a
temporary file in the same folder, flushed, then swapped in, so a failed save leaves the old file whole. The system's
save dialog asks before replacing a file.

## What it is careful about

**A crash.** A take whose JSON still says "recording" when the list is first read is repaired (`wav::repair`: the header
is set from the file's real length, a half sample at the end cut off) and listed as recovered. Only a header Recorder
itself writes (16-bit PCM) is touched; any other WAV is left alone. A take still held shut by another window of the
program is left to that window. Closing the window while recording finishes the take on the way out, and so does
quitting the program.

**Nothing is hidden.** If the disk stops taking writes, the take ends there and says it is incomplete. If the disk falls
more than two seconds behind and the ring fills, the samples that did not fit are counted, the take goes on, and it is
marked incomplete with the count. If the microphone goes away, what was recorded is kept and the take ends. A write
that failed is never counted as written.

**One recording at a time.** There is one microphone, so one recording for the whole program, made by the window that
started it. On Windows the take is opened with `FILE_SHARE_READ` only, so no other program, and no second Recorder, can
write to it while it records.

**Deleting.** Only takes in the recordings folder are ever deleted, never the one being recorded. The sound goes first,
and its JSON only once the sound is gone: a WAV another program is holding stays listed, with its JSON, so it can be
deleted later instead of becoming a file nobody lists.

**File names.** A take's file is named after its name, made safe for the file system and numbered when the name is
taken; the name it is listed by can differ from its file's.

## Windows and the Mac

- **Windows.** Each take opens in a program of its own, with one window. The window has no system title bar: the page
  draws the top bar and its buttons. `windows-app-manifest.xml` declares Windows 10 and 11 support, which Windows
  requires before it shows the snap layouts over the maximise button. The microphone is read through `cpal`.
- **The Mac.** One program shows each take in a window of its own, but there is still one recording at a time: only the
  window that started it pauses, marks or stops it, and another window meanwhile lists, plays, marks, saves and deletes
  finished takes. The first recording shows the system's question about the microphone (`mic_mac.rs`, with the reason in
  `NSMicrophoneUsageDescription` in `Info.plist`) and waits for the answer; on a no, the take does not start, because
  the microphone would only give silence. LAME is `libmp3lame.dylib`, beside the program or in the application's
  `Contents/Frameworks`. On the Mac another program holding a take never stops it being deleted.
