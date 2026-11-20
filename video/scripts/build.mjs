// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkDeps } from './check-deps.mjs';

// no codec may be compiled in
checkDeps();

await buildTool({
  root: join(dirname(fileURLToPath(import.meta.url)), '..'),
  name: 'DotAny Video',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-video',
    play: 'play',
    pause: 'pause',
    prev: 'chevron-left',
    next: 'chevron-right',
    volume: 'volume-2',
    mute: 'volume-x',
    full: 'maximize',
    unfull: 'minimize',
    subs: 'subtitles',
    cut: 'scissors',
    image: 'image-plus',
  },
});
