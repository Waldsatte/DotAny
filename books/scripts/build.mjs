// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const djvu = join(root, 'vendor/djvujs');
await buildTool({
  root,
  name: 'DotAny Books',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-text',
    toc: 'table-of-contents',
    type: 'a-large-small',
    search: 'search',
    up: 'chevron-up',
    down: 'chevron-down',
    close: 'x',
    bookmark: 'bookmark',
  },
  // DjVu.js isn't an npm package, so its licence is carried here, as its LICENSE.md asks
  notices: {
    before: [
      {
        title: 'DjVu.js 0.5.4 (GPL-2.0-or-later)',
        text: readFileSync(join(djvu, 'LICENSE.md'), 'utf8') + '\n\n' + readFileSync(join(djvu, 'GNU_GPL_v2'), 'utf8'),
      },
      {
        title: 'Noto Serif SC 2.003 (SIL OFL 1.1)',
        text: readFileSync(join(root, 'fonts/OFL-NotoSerifSC.txt'), 'utf8'),
      },
    ],
  },
});
// the reading serif is Books' own, not the family's: it ships inside this program (books/README.md), in the pieces
// scripts/split-serif.py cut it into, with their @font-face rules after the tool's sheet
if (process.env.DOTANY_NO_FONTS !== '1') {
  const pieces = join(root, 'fonts/serif');
  mkdirSync(join(root, 'dist/fonts/serif'), { recursive: true });
  for (const f of readdirSync(pieces).filter((n) => n.endsWith('.woff2')))
    copyFileSync(join(pieces, f), join(root, 'dist/fonts/serif', f));
  appendFileSync(join(root, 'dist/styles.css'), '\n' + readFileSync(join(pieces, 'faces.css'), 'utf8'));
}
// the DjVu decoder is fetched by the page only when a scan is opened (src/readers.js)
copyFileSync(join(djvu, 'djvu.js'), join(root, 'dist/djvu.js'));
