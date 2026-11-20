# Contributing to DotAny

English | [简体中文](#参与-dotany)

## What this repository has

Everything needed to build the programs: the code, the page builds, the fonts, the release workflow. The unit tests
inside the source files are part of this repository and run with `cargo test`. The end-to-end tests that drive the real
programs, and their sample files, are not part of this repository. That is why a pull request is not merged as it
stands: the change goes through those tests first, then arrives with the next snapshot of the code, and the pull request
is closed. This repository's history holds only those snapshots.

## Before you send a change

- Build what you changed, as the [README](../README.md#building-from-source) says.
- Lay out the JavaScript you changed with Prettier 3.10.1 (`npx prettier@3.10.1 --write <files>`; settings in
  `.prettierrc.json`) and the Rust with `cargo fmt` (settings in `rustfmt.toml`).
- The Check workflow runs on every pull request: each program's page builds, and both layout checks pass.
- Say what you changed and how you tried it. For a bug, attach a file that shows it, if you may share it.

## What is welcome, and what is not

Welcome: bug fixes, a file a tool opens wrongly, a crash, a wrong translation.

Not taken:

- A format or feature beyond what 95% of people meet and use. DotAny leaves the other 5% out on purpose.
- A change to the look: colours, type, spacing and motion are one design for the whole family.
- A library whose licence does not fit the code it goes into.

For anything larger than a fix, or a change to words on screen, open an issue first.

## Licence

What you send is published under the licence of the files it changes: MIT ([`LICENSE`](../LICENSE)), or
GPL-3.0-or-later inside `books/` ([`books/LICENSE`](../books/LICENSE)).

## Security

Report a security problem privately, through GitHub's private vulnerability reporting or to contact@dotany.app, not in
an issue.

---

# 参与 DotAny

[English](#contributing-to-dotany) | 简体中文

## 这个仓库里有什么

编出程序所需的一切：代码、页面构建、字体、发布用的工作流。写在源码文件里的单元测试在这个仓库里，用 `cargo test` 运行。
驱动真实程序的端到端测试和它们的样例文件不在这个仓库里。也因此拉取请求不会原样合并：改动先过那些测试，再随下一次代码
快照进来，拉取请求随之关闭。这个仓库的历史里只有这些快照。

## 提交之前

- 照 [README](../README.zh-CN.md#从源码构建) 把改过的部分编一遍。
- 改过的 JavaScript 用 Prettier 3.10.1 排版（`npx prettier@3.10.1 --write <文件>`，设置在 `.prettierrc.json`），Rust
  用 `cargo fmt`（设置在 `rustfmt.toml`）。
- 每个拉取请求都会跑 Check 工作流：每个程序的页面能构建，两项排版检查都通过。
- 写清楚改了什么、怎么试的。修问题时，能分享的话附上出问题的文件。

## 收什么，不收什么

收：修问题、工具打不开或打错的文件、崩溃、翻译错误。

不收：

- 超出 95% 原则的格式和功能。剩下的 5% 是有意不做的。
- 改样子：配色、字体、间距、动效是全家族共用的一套设计。
- 许可证和要放进去的代码不相容的库。

比修问题更大的改动，或者要改界面上的文字，先开一个议题。

## 许可

你提交的改动按它所改文件的许可发布：MIT（[`LICENSE`](../LICENSE)），`books/` 里是 GPL-3.0-or-later
（[`books/LICENSE`](../books/LICENSE)）。

## 安全问题

安全问题请私下报告：用 GitHub 的私密漏洞报告，或写信到 contact@dotany.app，不要开议题。
