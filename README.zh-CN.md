<div align="center">

<img src="hub/src-tauri/icons/icon.png" alt="DotAny" width="84" />

# DotAny

适用于几乎所有文件的开源编辑器。

[官网](https://dotany.app) · [下载](https://github.com/Waldsatte/DotAny/releases) · [问题反馈](https://github.com/Waldsatte/DotAny/issues) · [English](README.md)

<br />

<img src=".github/readme/demo.webp" alt="DotAny 演示" width="800" />

</div>

DotAny 将 11 款原生工具与 74 种文件格式整合于同一桌面套件。专注 95% 的核心需求，通过摒弃 5% 的低频冗余，换取每款工具的
瞬时启动与直观体验。

- 60 MB 安装包，完整安装约 164 MB
- 本地离线运行，免登录、无追踪，仅在检查更新时联网
- 支持 macOS（已通过苹果签名与公证）及 Windows
- 内置 9 种界面语言

## 支持格式（74 种）

| 工具 | 格式 |
| :--- | :--- |
| Text | txt、md、docx、doc、rtf、odt、json、yaml、toml、ini、xml、log、srt、vtt、ass、ssa、html、css、js、ts、py、java、c、cpp、go、rs、sh |
| Image | jpg、jpeg、png、webp、gif、svg、bmp、tiff、ico |
| Books | epub、fb2、cbz、djvu、mobi |
| Video | mp4、mkv、mov、avi、wmv |
| Audio | mp3、flac、wav、aiff、ogg、m4a |
| PDF | pdf |
| Archive | zip、7z、tar、gz、rar |
| Sheets | xlsx、csv、tsv、xls、ods、xlsm |
| Slides | pptx、ppsx、potx、pptm |
| 3D | glb、gltf、obj、stl、fbx、3mf |
| Recorder | 录制为 wav，可导出 wav、flac、mp3 |

部分格式仅支持查看（doc、rtf、odt、rar、xls、xlsm、pptm、m4a、avi、wmv）。各工具另兼容若干旧式与小众格式。

## 下载

- [DotAny.dmg](https://dotany.app/releases/latest/DotAny.dmg)（macOS 13 及以上，Apple 芯片；已签名并公证）
- [DotAny-Setup.exe](https://dotany.app/releases/latest/DotAny-Setup.exe)（Windows，64 位）

历史版本归档于 [Releases](https://github.com/Waldsatte/DotAny/releases)。

<details>
<summary>安装程序对系统的改动</summary>

- **默认应用（Windows）**：安装程序为各工具注册其支持的文件类型，将其加入「打开方式」，并把其中多数类型的默认应用设为
  DotAny。html、htm、ts、nc、db、mod、snd 仅注册，不设为默认；代码与数据文件（py、js、json、xml 等）仅在没有其他默认应用时
  归 DotAny；你在 Windows 中手动选定的默认应用始终保留。卸载时恢复原先的默认应用。如需查看或修改，可在中枢「默认应用」页
  跳转至对应的 Windows 设置。macOS 上由中枢「默认应用」页直接设置。
- **字体（Windows）**：安装程序为当前用户注册 HarmonyOS Sans SC 及繁体中文、日文、韩文字体，供所有 DotAny 窗口共用，
  其他应用的字体列表中也会出现。卸载时一并移除。
- **登录时启动**：在 Windows 与 macOS 上，DotAny 均随登录在后台启动，不显示窗口。如需关闭，请在中枢设置中关闭
  「开机时自动启动 DotAny」（Windows）或「登录时打开 DotAny」（macOS）；macOS 亦可在「系统设置 › 通用 › 登录项」中关闭。
- **安装位置**：Windows 为 `%LOCALAPPDATA%\Programs\DotAny`，仅限当前用户，无需管理员权限；可选完整安装或仅安装所需工具，
  中枢必装。macOS 将 dmg 中的 DotAny 文件夹拖入「应用程序」即可；卸载时将该文件夹移入废纸篓。

</details>

<details>
<summary>联网行为与更新校验</summary>

- **更新**：中枢在你点击「检查更新」时查询新版本，此外每天至多自动查询一次。新版本在后台下载（可在设置中关闭「自动下载
  更新」），仅在你点击「更新」后安装。安装前，中枢先以内置公钥验证发布中 SHA-256 清单的签名，再以该清单核对下载文件。
  查询顺序为 dotany.app、GitHub，仅限 HTTPS，且拒绝降级。此机制不等同于代码签名，首次运行时的 SmartScreen 提示仍会出现。
- **联系我们**：点击「发送」前不发送任何内容。发送时经 HTTPS 向 dotany.app 提交你填写的内容、中枢版本、界面语言与系统类型
  （Windows 或 macOS）。

</details>

<details>
<summary>校验下载文件</summary>

每个发布均附带 `SHA256SUMS.txt`、其签名及 `manifest.json`（版本号与安装包内各文件的 SHA-256）。计算下载文件的 SHA-256，
并与 `SHA256SUMS.txt` 中的对应行比对：

- PowerShell：`Get-FileHash .\DotAny-Setup.exe -Algorithm SHA256`
- macOS：`shasum -a 256 DotAny.dmg`

如需按中枢的方式校验 `SHA256SUMS.txt` 本身，可使用 [minisign](https://jedisct1.github.io/minisign/) 与中枢内置公钥
（`hub/src-tauri/src/update.rs` 中的 `TRUSTED_KEYS`）验证其签名：

```
minisign -Vm SHA256SUMS.txt -x SHA256SUMS.txt.sig -P RWRTNXn4HhYmDLLt259wSyXBPb3H1itR3a+ApiZcvVPp3uX3J00Rjkca
```

Windows 安装包另附 GitHub 构建来源证明：`gh attestation verify DotAny-Setup.exe --repo Waldsatte/DotAny`。
macOS 安装包经 Apple 公证：`spctl -a -t open --context context:primary-signature -v DotAny.dmg`。

</details>

## 从源码构建

各工具均为独立的 [Tauri 2](https://tauri.app) 应用，位于各自目录。构建需 [Node.js](https://nodejs.org) 与
[Rust](https://rustup.rs)（Windows 另需 MSVC 工具链）。构建单个工具，以 Text 为例：

```bash
git clone https://github.com/Waldsatte/DotAny.git
cd DotAny/text
npm ci
npx tauri build --no-bundle
```

产物为 `src-tauri/target/release/dotany-text.exe`，可原地运行。

<details>
<summary>构建全部工具、安装包与 macOS 版</summary>

各工具由 `src/` 中的页面（esbuild 打包）与 `src-tauri/` 中的 Rust 程序组成。任意当前稳定版 Rust 均可构建；发布流程通过
`RUST_TOOLCHAIN` 固定版本，并使用 Node 22。Windows 上的运行环境为 Microsoft Edge WebView2。

全部程序与安装包：在仓库根目录使用 Git Bash。`CARGO_TARGET_DIR` 为全部程序共用的 Cargo 输出目录，`DOTANY_BUILD` 为成品
的汇集目录，供安装包打包（见 [`.github/scripts/build-programs.sh`](.github/scripts/build-programs.sh)）：

```bash
export CARGO_TARGET_DIR=C:/build/target DOTANY_BUILD=C:/build/programs
bash .github/scripts/build-programs.sh windows
cargo build --release --manifest-path installer/src-tauri/Cargo.toml
mkdir -p "$DOTANY_BUILD/cargo-target-installer/release" out
cp "$CARGO_TARGET_DIR/release/dotany-setup.exe" "$DOTANY_BUILD/cargo-target-installer/release/"
"$CARGO_TARGET_DIR/release/dotany-pack.exe" --repo "$PWD" --out "$PWD/out"
```

以上与发布流程一致（[`.github/workflows/release.yml`](.github/workflows/release.yml)）。完成后 `out/` 中为
`DotAny-Setup.exe` 与 `manifest.json`。安装程序与打包工具的说明见 [`installer/README.md`](installer/README.md)。

macOS（Apple 芯片，需 Xcode 命令行工具、Node.js 与 Rust）：在各程序目录执行 `npm ci`，随后运行
`node shared/scripts/mac-release.mjs hub text image audio video pdf archive sheets slides books model recorder`，再运行
`node shared/scripts/mac-release.mjs --package` 生成 `DotAny.dmg`。详见该脚本开头的注释。

单元测试位于源码文件内，通过 `cargo test` 运行。端到端测试及其样例文件不包含在本仓库中。

</details>

<details>
<summary>仓库结构</summary>

| 目录 | 内容 |
| :--- | :--- |
| `text/`、`image/`、`audio/`、`video/`、`pdf/`、`archive/`、`sheets/`、`slides/`、`books/`、`model/`（3D）、`recorder/` | 各对应一款工具：`src/`（页面）、`src-tauri/`（Rust 程序）、`scripts/`（页面构建） |
| `hub/` | 中枢 DotAny：新建、最近打开、格式转换、工具的安装与更新、默认应用 |
| `installer/` | 全套件唯一的安装程序 `DotAny-Setup.exe`，以及为其打包的 `dotany-pack` |
| `shared/` | 公共基础：`web/`（窗口、组件、样式表、各语言词表）、`rust/`（`dotany-base`：保存、设置、最近打开、窗口）、多款工具共用的读写模块（`picture/`、`sound/`、`sound-encode/`、`table/`、`unpack/`、`repack/`、`words/`）、`convert/`（中枢的转换器）、`fonts/`、`scripts/`（页面构建、许可声明、macOS 打包）、`upstream-licences/`、`starter/`（新工具模板） |
| `.github/` | 发布与检查流程及其脚本 |

各程序目录均附有面向开发者的 `README.md`（英文）。公共基础的说明见 [`shared/README.md`](shared/README.md)（英文）。

</details>

## 开源致谢

基于 [Tauri](https://tauri.app) 构建，底层集成 CodeMirror、ProseMirror、PDF.js、lopdf、three.js、Symphonia、calamine、
libarchive、sevenz-rust2 等开源组件。完整许可协议见应用内「设置 › 关于 › 开源许可与字体协议」。

## 反馈与贡献

问题反馈与新格式需求请提交至 [Issues](https://github.com/Waldsatte/DotAny/issues)，代码贡献请提交
[Pull Requests](https://github.com/Waldsatte/DotAny/pulls)，贡献指南见 [CONTRIBUTING.md](.github/CONTRIBUTING.md)。
安全问题请通过 GitHub 私密漏洞报告或 contact@dotany.app 私下提交。

## 许可协议

MIT。DotAny Books（`books/`）因集成 DjVu.js，采用 GPL-3.0-or-later。字体与第三方代码沿用各自的许可协议（字体为
SIL OFL 1.1，HarmonyOS Sans 适用华为的许可协议）。详见 [LICENSE](LICENSE)。
