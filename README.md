<div align="center">

<img src="hub/src-tauri/icons/icon.png" alt="DotAny" width="84" />

# DotAny

An open-source editor for almost any file.

[Website](https://dotany.app) · [Download](https://github.com/Waldsatte/DotAny/releases) · [Issues](https://github.com/Waldsatte/DotAny/issues) · [简体中文](README.zh-CN.md)

<br />

<img src=".github/readme/demo.webp" alt="DotAny demo" width="800" />

</div>

DotAny brings 11 native tools and 74 file formats into a single desktop suite. Built for the 95%. By leaving out the
bloated 5%, every tool launches instantly and stays intuitive.

- 60 MB installer (~164 MB installed)
- Runs offline, with no accounts or tracking; connects only to check for updates
- Signed and notarized on macOS; available for Windows
- Localized in 9 languages

## Supported Formats (74)

| Tool | Formats |
| :--- | :--- |
| Text | txt, md, docx, doc, rtf, odt, json, yaml, toml, ini, xml, log, srt, vtt, ass, ssa, html, css, js, ts, py, java, c, cpp, go, rs, sh |
| Image | jpg, jpeg, png, webp, gif, svg, bmp, tiff, ico |
| Books | epub, fb2, cbz, djvu, mobi |
| Video | mp4, mkv, mov, avi, wmv |
| Audio | mp3, flac, wav, aiff, ogg, m4a |
| PDF | pdf |
| Archive | zip, 7z, tar, gz, rar |
| Sheets | xlsx, csv, tsv, xls, ods, xlsm |
| Slides | pptx, ppsx, potx, pptm |
| 3D | glb, gltf, obj, stl, fbx, 3mf |
| Recorder | Records to wav; exports wav, flac, mp3 |

Some formats are view-only (doc, rtf, odt, rar, xls, xlsm, pptm, m4a, avi, wmv). Each tool also reads a range of legacy
and niche formats.

## Download

- [DotAny.dmg](https://dotany.app/releases/latest/DotAny.dmg) (macOS 13 or later, Apple silicon; signed and notarized)
- [DotAny-Setup.exe](https://dotany.app/releases/latest/DotAny-Setup.exe) (Windows, 64-bit)

Previous builds are archived in [Releases](https://github.com/Waldsatte/DotAny/releases).

<details>
<summary>What the installer changes</summary>

- **Default apps (Windows).** The installer registers each tool for its supported file types, adds it to Open with,
  and sets DotAny as the default for most of them. html, htm, ts, nc, db, mod, and snd are registered only. Code and
  data files (py, js, json, xml, etc.) default to DotAny only if no other default exists. Defaults you chose manually
  in Windows are never changed. Uninstalling restores the previous defaults. To review or change them, open the hub's
  Default Apps page, which links to the matching Windows settings. On macOS, the hub's Default Apps page sets them
  directly.
- **Fonts (Windows).** The installer registers HarmonyOS Sans SC, along with the Traditional Chinese, Japanese, and
  Korean fonts, for the current user. All DotAny windows share them, and they appear in other apps' font lists.
  Uninstalling removes them.
- **Launch at login.** On Windows and macOS, DotAny starts in the background at login without showing a window. To
  disable it, turn off **Launch DotAny at login** (Windows) or **Open DotAny at login** (macOS) in the hub's Settings,
  or use System Settings › General › Login Items on macOS.
- **Install location.** Windows: `%LOCALAPPDATA%\Programs\DotAny`, current user only, no administrator rights
  required. Choose a full install or individual tools; the hub is always included. macOS: drag the DotAny folder from
  the dmg to Applications. To uninstall, move that folder to the Trash.

</details>

<details>
<summary>Network activity and update verification</summary>

- **Updates.** The hub checks for a new release when you click Check for Updates, and automatically at most once a
  day. New releases download in the background (disable under Settings › Auto-download updates) and install only when
  you click Update. Before installing, the hub verifies the signature of the release's SHA-256 list against a built-in
  public key, then verifies the download against that list. It queries dotany.app first, then GitHub, over HTTPS only,
  and rejects downgrades. This is not code signing and does not suppress the first-run SmartScreen prompt.
- **Contact Us.** Nothing is sent until you click Send. The message, hub version, interface language, and OS (Windows
  or macOS) are then sent to dotany.app over HTTPS.

</details>

<details>
<summary>Verifying a download</summary>

Each release includes `SHA256SUMS.txt`, its signature, and `manifest.json` (the version and SHA-256 of every file in
the installer). Compare the download's SHA-256 against its entry in `SHA256SUMS.txt`:

- PowerShell: `Get-FileHash .\DotAny-Setup.exe -Algorithm SHA256`
- macOS: `shasum -a 256 DotAny.dmg`

To verify `SHA256SUMS.txt` itself the way the hub does, check its signature with
[minisign](https://jedisct1.github.io/minisign/) and the hub's built-in public key (`TRUSTED_KEYS` in
`hub/src-tauri/src/update.rs`):

```
minisign -Vm SHA256SUMS.txt -x SHA256SUMS.txt.sig -P RWRTNXn4HhYmDLLt259wSyXBPb3H1itR3a+ApiZcvVPp3uX3J00Rjkca
```

Releases built in this repository also include GitHub build provenance:
`gh attestation verify DotAny-Setup.exe --repo Waldsatte/DotAny` (or `DotAny.dmg`).

</details>

## Build from Source

Each tool is a standalone [Tauri 2](https://tauri.app) app in its own directory. Requires
[Node.js](https://nodejs.org) and [Rust](https://rustup.rs) (plus the MSVC toolchain on Windows). To build a single
tool, e.g. Text:

```bash
git clone https://github.com/Waldsatte/DotAny.git
cd DotAny/text
npm ci
npx tauri build --no-bundle
```

The binary is written to `src-tauri/target/release/dotany-text.exe` and runs in place.

<details>
<summary>Building all tools, the installer, and the macOS package</summary>

Each tool consists of a web page in `src/` (bundled with esbuild) and a Rust program in `src-tauri/`. Any current
stable Rust works; the release workflow pins a version via `RUST_TOOLCHAIN` and uses Node 22. On Windows, the apps run
on Microsoft Edge WebView2.

All programs plus the installer, in Git Bash from the repository root. `CARGO_TARGET_DIR` is a Cargo target directory
shared by all programs; `DOTANY_BUILD` is where the built programs are collected for packaging (see
[`.github/scripts/build-programs.sh`](.github/scripts/build-programs.sh)):

```bash
export CARGO_TARGET_DIR=C:/build/target DOTANY_BUILD=C:/build/programs
bash .github/scripts/build-programs.sh windows
cargo build --release --manifest-path installer/src-tauri/Cargo.toml
mkdir -p "$DOTANY_BUILD/cargo-target-installer/release" out
cp "$CARGO_TARGET_DIR/release/dotany-setup.exe" "$DOTANY_BUILD/cargo-target-installer/release/"
"$CARGO_TARGET_DIR/release/dotany-pack.exe" --repo "$PWD" --out "$PWD/out"
```

This mirrors the release workflow ([`.github/workflows/release.yml`](.github/workflows/release.yml)).
The output in `out/` is `DotAny-Setup.exe` and `manifest.json`. See [`installer/README.md`](installer/README.md) for
the installer and its packer.

macOS (Apple silicon; requires Xcode Command Line Tools, Node.js, and Rust): run `npm ci` in each program's directory,
then `node shared/scripts/mac-release.mjs hub text image audio video pdf archive sheets slides books model recorder`,
then `node shared/scripts/mac-release.mjs --package` to produce `DotAny.dmg`. See the comment at the top of that script
for details.

Unit tests live alongside the source and run with `cargo test`. End-to-end tests and their sample files are not
included in this repository.

</details>

<details>
<summary>Repository layout</summary>

| Directory | Contents |
| :--- | :--- |
| `text/`, `image/`, `audio/`, `video/`, `pdf/`, `archive/`, `sheets/`, `slides/`, `books/`, `model/` (3D), `recorder/` | One tool each: `src/` (web page), `src-tauri/` (Rust program), `scripts/` (page build) |
| `hub/` | The DotAny hub: new files, recent files, converter, tool installation and updates, default apps |
| `installer/` | `DotAny-Setup.exe`, the single installer for the suite, and `dotany-pack`, which packages it |
| `shared/` | Common foundation: `web/` (window, components, stylesheet, translations), `rust/` (`dotany-base`: saving, settings, recent files, window), shared readers and writers (`picture/`, `sound/`, `sound-encode/`, `table/`, `unpack/`, `repack/`, `words/`), `convert/` (the hub's converter), `fonts/`, `scripts/` (page build, license notices, macOS packaging), `upstream-licences/`, `starter/` (template for a new tool) |
| `.github/` | Release and check workflows and their scripts |

Each program's directory has its own developer `README.md`. See [`shared/README.md`](shared/README.md) for the
foundation.

</details>

## Built On

Built with [Tauri](https://tauri.app), integrating CodeMirror, ProseMirror, PDF.js, lopdf, three.js, Symphonia,
calamine, libarchive, sevenz-rust2, and other open-source libraries. Full license texts are bundled in the app under
Settings › About › Licenses & Credits.

## Feedback

Report bugs or request format support in [Issues](https://github.com/Waldsatte/DotAny/issues). Submit code
contributions via [Pull Requests](https://github.com/Waldsatte/DotAny/pulls); see
[CONTRIBUTING.md](.github/CONTRIBUTING.md) for guidelines. Report security issues privately via GitHub's private
vulnerability reporting or contact@dotany.app.

## License

MIT. DotAny Books (`books/`) is licensed under GPL-3.0-or-later because it bundles DjVu.js. Fonts and third-party code
retain their own licenses (fonts: SIL OFL 1.1; HarmonyOS Sans: Huawei's license agreement). See [LICENSE](LICENSE).
