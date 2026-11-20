// DotAny Starter: the family's window around an empty work area. A new tool copies this folder and draws its work in
// `shell.stage`. Everything around it, from the top bar to the settings panel, is shared/web.
import { createShell } from '@dotany/web/shell.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';

// the tool's own words in other languages: src/lang/<code>.json, added to the family's (shared/web/lang) by createShell
import en from './lang/en.json';
// English is always here; every other language is loaded when chosen (shared/web/i18n.js LANGS lists them)
const WORDS = {
  en,
  'zh-Hant': () => import('./lang/zh-Hant.json'),
  ja: () => import('./lang/ja.json'),
  ko: () => import('./lang/ko.json'),
  'es-419': () => import('./lang/es-419.json'),
  'pt-BR': () => import('./lang/pt-BR.json'),
  fr: () => import('./lang/fr.json'),
  de: () => import('./lang/de.json'),
};
// the save card's formats; the extension is shown beside the name, not written into the format's name
const FORMATS = { txt: { ext: '.txt', label: 'TXT' }, md: { ext: '.md', label: 'Markdown' } };

// what's open, and how many changes since it was saved. The starter has no edits of its own: a tool counts its own here.
let edits = 0,
  savedAt = 0;

const shell = createShell({
  name: 'DotAny Starter',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽文件至此',
  fileKind: '文本文件',
  formats: FORMATS,
  formatOf: (doc) => (/\.md$/i.test(doc.name) ? 'md' : 'txt'),
  open: own.openFile,
  show() {
    edits = 0;
    savedAt = 0;
  },
  clear() {
    edits = 0;
    savedAt = 0;
  },
  dirty: () => edits !== savedAt,
  discard() {
    savedAt = edits;
  },
  estimate: () => own.estimateSave(),
  save: (path) => own.saveFile(path),
  saved() {
    savedAt = edits;
  },
});

// with DOTANY_TEST set, shell.test.touch makes a change the way a tool's own editing would, so that an automated test
// can check the unsaved-changes handling
shell.test.touch = () => {
  edits++;
  shell.changed();
};
shell.begin();
