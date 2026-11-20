// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../shared/scripts/build.mjs';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
await buildTool({
  root,
  name: 'DotAny Archive',
  // the recent-files rows' icon, then the tool's own buttons and the list's file kinds
  icons: {
    file: 'file-archive',
    eye: 'eye',
    eyeOff: 'eye-off',
    search: 'search',
    up14: 'chevron-up',
    add: 'file-plus',
    folderPlus: 'folder-plus',
    rename: 'pencil-line',
    trash: 'trash-2',
    undo: 'undo-2',
    redo: 'redo-2',
    close: 'x',
    lock: 'lock',
    folder: 'folder',
    doc: 'file',
    image: 'file-image',
    code: 'file-code',
    sheet: 'file-spreadsheet',
    text: 'file-text',
    video: 'file-video',
    music: 'file-music',
    pack: 'file-archive',
  },
  // libarchive is BSD: its notice travels with the program. Only its RAR, LHA, cpio and ar readers are compiled in, by
  // dotany-unpack (shared/unpack/build.rs).
  notices: {
    before: [
      {
        title: 'libarchive 3.8.11, its RAR 4, RAR 5, LHA, cpio and ar readers (read only)',
        text:
          "DotAny Archive reads RAR, LHA, cpio and ar files (and so .deb and .rpm packages) with libarchive (https://libarchive.org). Only its reading core and those readers are compiled in; the source is in the folder shared/unpack/vendor/libarchive of DotAny's source.\n\n"
          + readFileSync(join(root, '../shared/unpack/vendor/libarchive/COPYING'), 'utf8'),
      },
    ],
  },
});
