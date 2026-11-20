// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkDeps } from './check-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
// the formats DotAny Audio leaves out stay out: no code for them is ever compiled in (check-deps.mjs)
console.log('deps', JSON.stringify(checkDeps()));

// the C libraries' sources are in the crates that build them: shared/sound-encode (LAME, libFLAC) and shared/sound (libopenmpt)
const vendor = (crate, f) => readFileSync(join(root, '../shared', crate, 'vendor', f), 'utf8');
await buildTool({
  root,
  name: 'DotAny Audio',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-audio',
    play: 'play',
    pause: 'pause',
    prev: 'skip-back',
    next: 'skip-forward',
    adjust: 'audio-lines',
    undo: 'undo-2',
    redo: 'redo-2',
    trash: 'trash-2',
    crop: 'crop',
    image: 'image-plus',
  },
  // what LAME's, libFLAC's and libopenmpt's licences ask to be said about how they ship, before the fonts
  notices: {
    before: [
      {
        title: 'LAME 3.100, the MP3 encoder in libmp3lame.dll',
        text:
          "This program uses LAME (https://lame.sourceforge.io), under the GNU Lesser General Public License, printed below.\nIts source code is in the folder shared/sound-encode/vendor/lame-3.100 of DotAny's source, and is also available from the address above.\nLAME's decoder is not included. LAME was not modified, apart from leaving those files out.\nlibmp3lame.dll sits beside the program and is loaded when an MP3 is saved: you may replace it with your own build of LAME.\n\n"
          + vendor('sound-encode', 'lame-3.100/COPYING'),
      },
      { title: 'libFLAC 1.5.0', text: vendor('sound-encode', 'flac/COPYING.Xiph') },
      {
        title: 'libopenmpt 0.8.9, which plays MOD, XM, S3M and IT',
        text:
          "Built from the source in the folder shared/sound/vendor/libopenmpt of DotAny's source (https://lib.openmpt.org), without the third-party decoders its release carries. It was not modified.\n\n"
          + vendor('sound', 'libopenmpt/LICENSE'),
      },
    ],
  },
});
