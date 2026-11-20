// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here: its
// icons, Lucide drawings by name.
import { buildTool } from '../../shared/scripts/build.mjs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

await buildTool({
  root: join(dirname(fileURLToPath(import.meta.url)), '..'),
  name: 'DotAny Slides',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'presentation',
    grid: 'layout-grid',
    notes: 'sticky-note',
    prev: 'chevron-left',
    next: 'chevron-right',
    up: 'chevron-up',
    down: 'chevron-down',
    close: 'x',
    play: 'play',
    laser: 'circle-dot',
    black: 'monitor-off',
    presenter: 'presentation',
    hidden: 'eye-off',
    undo: 'undo-2',
    redo: 'redo-2',
    type: 'type',
    image: 'image',
    shapes: 'shapes',
    film: 'film',
    anim: 'sparkles',
    copy: 'copy',
    trash: 'trash-2',
    plus: 'plus',
    minus: 'minus',
    bold: 'bold',
    italic: 'italic',
    underline: 'underline',
    alignL: 'align-left',
    alignC: 'align-center',
    alignR: 'align-right',
    bullets: 'list',
    textColor: 'baseline',
    fill: 'paint-bucket',
    line: 'pen-line',
    front: 'bring-to-front',
    backward: 'send-to-back',
  },
});
