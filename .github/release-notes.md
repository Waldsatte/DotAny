## Install (Windows)

1. Download **DotAny-Setup.exe** below and run it. It installs for the current user, no administrator rights needed.
2. The installer is **not code-signed**. Windows SmartScreen may say "Windows protected your PC":
   choose **More info > Run anyway**. To check the file first, compare its SHA-256 with `SHA256SUMS.txt`:
   `Get-FileHash .\DotAny-Setup.exe -Algorithm SHA256` (PowerShell).
3. Optional: `gh attestation verify DotAny-Setup.exe --repo Waldsatte/DotAny` shows that GitHub built this file from this
   repository.

## 安装（Windows）

1. 下载下面的 **DotAny-Setup.exe**，双击运行。装在当前用户名下，不需要管理员权限。
2. 安装包**没有代码签名**，Windows 可能弹出「Windows 已保护你的电脑」：点「更多信息」，再点「仍要运行」。
   想先核对文件：在 PowerShell 里运行 `Get-FileHash .\DotAny-Setup.exe -Algorithm SHA256`，结果应与 `SHA256SUMS.txt` 里那行一致。
3. 可选：`gh attestation verify DotAny-Setup.exe --repo Waldsatte/DotAny` 能证明这个文件是 GitHub 用这个仓库构建出来的。

## macOS

The Mac version is **DotAny.dmg** (Mac with Apple silicon, macOS 13 or later): open it and drag the **DotAny** folder
onto **Applications**. The programs are signed with an Apple Developer ID and notarised by Apple. Every program is in
that folder; to remove DotAny, drag the folder from Applications to the Trash.

## macOS（中文）

Mac 版是 **DotAny.dmg**（Apple 芯片的 Mac，macOS 13 或更高版本）：打开它，把「DotAny」文件夹拖到「应用程序」。这些程序用
Apple 开发者 ID 签了名，并经过 Apple 公证。所有程序都在这个文件夹里；要删掉 DotAny，把「应用程序」里的这个文件夹拖到废纸篓。
