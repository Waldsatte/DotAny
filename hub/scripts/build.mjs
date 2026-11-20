// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is DotAny's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { convertNotices } from '../../shared/convert/notices.mjs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

await buildTool({
  root: join(dirname(fileURLToPath(import.meta.url)), '..'),
  name: 'DotAny',
  // the places, the converter; the tools' glyphs are their app icons'
  icons: {
    plus: 'plus',
    search: 'search',
    home: 'house',
    check: 'list-checks',
    convert: 'arrow-left-right',
    tools: 'blocks',
    close: 'x',
    doc: 'file-text',
    sheet: 'table-2',
    slide: 'presentation',
    image: 'image',
    film: 'webcam',
    music: 'disc-3',
    archive: 'archive',
    box: 'orbit',
    book: 'book-image',
    pdf: 'pen-tool',
    mic: 'circle-dot',
    gone: 'file-question',
    mail: 'mail',
    copy: 'copy',
    tick: 'check',
    arrow: 'arrow-right',
    file: 'file',
    // Default Apps: a tool's own page in Windows' settings, PDF's icon for a link that opens outside
    external: 'external-link',
  },
  // the converter's C libraries (LAME, libFLAC, libarchive, libwebp): their texts come with it; its Rust crates are found by themselves
  notices: { before: convertNotices() },
});
