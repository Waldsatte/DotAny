// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

await buildTool({
  root: join(dirname(fileURLToPath(import.meta.url)), '..'),
  name: 'DotAny Sheets',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-spreadsheet',
    headerRow: 'panel-top',
    filter: 'list-filter',
    search: 'search',
    undo: 'undo-2',
    redo: 'redo-2',
    close: 'x',
    up: 'chevron-up',
    down: 'chevron-down',
    next: 'chevron-right',
  },
});
