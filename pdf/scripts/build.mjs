// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here:
// PDF.js's worker beside the page, with HTJ2K turned off in it (src/jpx-guard.js), and PDF.js's character maps and
// standard fonts, which it reads when a file doesn't carry its own.
import { buildTool, fromTool } from '../../shared/scripts/build.mjs';
import { cpSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const pdfjs = join(root, 'node_modules/pdfjs-dist');
const text = (p) => readFileSync(p, 'utf8');

await buildTool({
  root,
  name: 'DotAny PDF',
  // the recent-files rows' icon, then the tool's own buttons
  icons: {
    file: 'file-text',
    edit: 'pencil-line',
    bold: 'bold',
    toc: 'table-of-contents',
    search: 'search',
    grid: 'layout-grid',
    highlight: 'highlighter',
    brush: 'brush',
    type: 'type',
    sign: 'signature',
    redact: 'eye-off',
    undo: 'undo-2',
    redo: 'redo-2',
    prev: 'chevron-left',
    next: 'chevron-right',
    minus: 'minus',
    plus: 'plus',
    rotateL: 'rotate-ccw',
    rotateR: 'rotate-cw',
    trash: 'trash-2',
    extract: 'file-output',
    add: 'file-plus-2',
    copy: 'copy',
    up: 'chevron-up',
    down: 'chevron-down',
    close: 'x',
    external: 'external-link',
  },
  // what ships inside PDF.js's worker and beside it, with no package of its own to carry its licence
  notices: {
    before: [
      {
        title: "OpenJPEG 2.5 (inside PDF.js's worker, for JPEG 2000 pictures; HTJ2K turned off)",
        text: text(join(root, 'scripts/OpenJPEG-LICENSE.txt')),
      },
      { title: 'PDF.js standard fonts: Foxit (from PDFium)', text: text(join(pdfjs, 'standard_fonts/LICENSE_FOXIT')) },
      { title: 'PDF.js standard fonts: Liberation', text: text(join(pdfjs, 'standard_fonts/LICENSE_LIBERATION')) },
    ],
  },
});

// the worker: PDF.js's own, with the guard put in front of the JPEG 2000 decoder
const ANCHOR = 'static decode(data, decoderOptions) {';
let worker = text(join(pdfjs, 'build/pdf.worker.mjs'));
const at = worker.indexOf(ANCHOR);
if (at < 0 || worker.indexOf(ANCHOR, at + 1) >= 0)
  throw new Error('PDF.js changed: JpxImage.decode not found once in the worker');
const guard = text(join(root, 'src/jpx-guard.js')).replace('export function isHtj2k(', 'function dotanyIsHtj2k(');
if (!guard.includes('function dotanyIsHtj2k(')) throw new Error('src/jpx-guard.js changed: isHtj2k not found');
worker =
  worker.slice(0, at)
  + ANCHOR
  + '\n    if (dotanyIsHtj2k(data)) throw new JpxError("HTJ2K (JPEG 2000 Part 15) is turned off");'
  + worker.slice(at + ANCHOR.length);
worker = guard + '\n' + worker;
const { build } = await fromTool(root, 'esbuild');
const out = await build({
  stdin: { contents: worker, loader: 'js', resolveDir: join(pdfjs, 'build') },
  bundle: true,
  format: 'esm',
  minify: true,
  target: 'es2022',
  write: false,
  legalComments: 'none',
  charset: 'utf8',
});
writeFileSync(join(root, 'dist/pdf.worker.js'), out.outputFiles[0].text);
cpSync(join(pdfjs, 'cmaps'), join(root, 'dist/cmaps'), { recursive: true });
cpSync(join(pdfjs, 'standard_fonts'), join(root, 'dist/standard_fonts'), { recursive: true });
console.log('pdf.worker.js', Math.round(out.outputFiles[0].text.length / 1024) + 'KB');
