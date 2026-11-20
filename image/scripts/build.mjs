// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkDeps } from './check-deps.mjs';

// the formats DotAny Image leaves out stay out: no code for them is ever compiled in (check-deps.mjs)
checkDeps();

await buildTool({
  root: join(dirname(fileURLToPath(import.meta.url)), '..'),
  name: 'DotAny Image',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-image',
    crop: 'crop',
    adjust: 'contrast',
    brush: 'brush',
    type: 'type',
    rotateL: 'rotate-ccw',
    rotateR: 'rotate-cw',
    flipH: 'flip-vertical-2',
    flipV: 'flip-horizontal-2',
    undo: 'undo-2',
    redo: 'redo-2',
    prev: 'chevron-left',
    next: 'chevron-right',
    minus: 'minus',
    plus: 'plus',
    play: 'play',
    pause: 'pause',
    tick: 'check',
  },
});
