// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
// LAME's and libFLAC's sources are in the crate that builds them, shared with Audio
const vendor = (f) => readFileSync(join(root, '../shared/sound-encode/vendor', f), 'utf8');
await buildTool({
  root,
  name: 'DotAny Recorder',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-audio',
    list: 'list-music',
    mic: 'mic',
    audio: 'audio-lines',
    start: 'skip-back',
    play: 'play',
    pause: 'pause',
    flag: 'flag',
    trash: 'trash-2',
  },
  // what LAME's and libFLAC's licences ask to be said about how they ship, with their licence texts
  notices: {
    before: [
      {
        title: 'LAME 3.100, the MP3 encoder in libmp3lame.dll',
        text:
          "This program uses LAME (https://lame.sourceforge.io), under the GNU Lesser General Public License, printed below.\nIts source code is in the folder shared/sound-encode/vendor/lame-3.100 of DotAny's source, and is also available from the address above.\nLAME's decoder is not included. LAME was not modified, apart from leaving those files out.\nlibmp3lame.dll sits beside the program and is loaded when an MP3 is saved: you may replace it with your own build of LAME.\n\n"
          + vendor('lame-3.100/COPYING'),
      },
      { title: 'libFLAC 1.5.0', text: vendor('flac/COPYING.Xiph') },
    ],
  },
});
