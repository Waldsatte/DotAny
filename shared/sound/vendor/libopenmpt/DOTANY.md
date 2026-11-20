# libopenmpt 0.8.9 in DotAny Audio

- Source: https://lib.openmpt.org/files/libopenmpt/src/libopenmpt-0.8.9+release.makefile.tar.gz
  (SHA-256 9273b88b67973cc69e54d748ab1b749399d6d07695f1c37d0c59f88b4106074f), downloaded 2026-11-13.
- Licence: BSD-3-Clause, `LICENSE` here. Printed in Audio's licence page by `audio/scripts/build.mjs`.
- Kept, unmodified: `LICENSE`, `README.md`, `common/`, `soundlib/`, `sounddsp/`, `src/` and the library's own files from
  `libopenmpt/` (headers, `libopenmpt_c.cpp`, `libopenmpt_cxx.cpp`, `libopenmpt_impl.cpp`, `libopenmpt_ext_impl.cpp`).
- Left out: `include/` (minimp3, stb_vorbis, miniz and the other third-party code the release carries), `contrib/`,
  `examples/`, `openmpt123/`, `test/`, `build/` (all but `build/svn_version/svn_version.h`, the release's version
  stamp, which `common/version.cpp` includes), `doc/`, and the player plugins and bindings under `libopenmpt/`.
- Built by `shared/sound/build.rs` (`build_openmpt`), once for Audio and the hub's converter: C++17, `LIBOPENMPT_BUILD` (and `UNICODE` on Windows, as its own Windows builds) only, no `MPT_WITH_*` switch, so no
  zlib, MP3, Vorbis, Ogg, FLAC, Opus or Media Foundation code. `audio/scripts/check-deps.mjs` fails the build otherwise.
- Audio lets only MOD, XM, S3M and IT reach it (`formats.rs` and `module.rs`).
