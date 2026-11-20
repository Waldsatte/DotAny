// What the converter adds to a program's licence page beyond what shared/scripts/notices.mjs finds by itself. That
// script walks the Rust crates really compiled in, dotany-convert's included, so every crate's licence is already there.
// It can't see C code: LAME and libFLAC are built from shared/sound-encode/vendor, libopenmpt from shared/sound/vendor,
// libarchive from shared/unpack/vendor, and libwebp comes inside the libwebp-sys crate, whose own licence (MIT) is only
// the wrapper's. Their texts are here, as Recorder and Archive give theirs. A program using the converter passes these
// as `before`:
//
//   import { convertNotices } from '../../shared/convert/notices.mjs';
//   await generate({ root, before: [...convertNotices(), ...ownNotices] });
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
// the C libraries' sources, in the crates that build them once for the tools and the converter
const sound = (crate, p) => readFileSync(join(HERE, '..', crate, 'vendor', p), 'utf8');

function libwebp() {
  const meta = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'], {
      cwd: HERE,
      encoding: 'utf8',
      maxBuffer: 2e8,
    })
  );
  const pkg = meta.packages.find((p) => p.name === 'libwebp-sys');
  const dir = join(dirname(pkg.manifest_path), 'vendor');
  return {
    version: pkg.version,
    copying: readFileSync(join(dir, 'COPYING'), 'utf8'),
    patents: readFileSync(join(dir, 'PATENTS'), 'utf8'),
  };
}

export function convertNotices() {
  const webp = libwebp();
  return [
    {
      title: 'LAME 3.100, the MP3 encoder in libmp3lame.dll',
      text:
        "This program uses LAME (https://lame.sourceforge.io), under the GNU Lesser General Public License, printed below.\nIts source code is in the folder shared/sound-encode/vendor/lame-3.100 of DotAny's source, and is also available from the address above.\nLAME's decoder is not included. LAME was not modified, apart from leaving those files out.\nlibmp3lame.dll sits beside the program and is loaded when an MP3 is saved: you may replace it with your own build of LAME.\n\n"
        + sound('sound-encode', 'lame-3.100/COPYING'),
    },
    { title: 'libFLAC 1.5.0', text: sound('sound-encode', 'flac/COPYING.Xiph') },
    {
      title: 'libopenmpt 0.8.9, which plays MOD, XM, S3M and IT',
      text:
        "Built from the source in the folder shared/sound/vendor/libopenmpt of DotAny's source (https://lib.openmpt.org), without the third-party decoders its release carries. It was not modified.\n\n"
        + sound('sound', 'libopenmpt/LICENSE'),
    },
    {
      title: 'libarchive 3.8.11, its RAR 4 and RAR 5 readers (read only)',
      text:
        "This program reads RAR files with libarchive (https://libarchive.org). Only its reading core and its two RAR readers are compiled in; the source is in the folder shared/unpack/vendor/libarchive of DotAny's source.\n\n"
        + sound('unpack', 'libarchive/COPYING'),
    },
    { title: `libwebp (inside libwebp-sys ${webp.version})`, text: webp.copying + '\n' + webp.patents },
  ];
}
