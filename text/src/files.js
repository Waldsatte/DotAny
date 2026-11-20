// The real file's life in a window: opening, the status line, saving, closing, notices, printing.
import { TextSelection } from 'prosemirror-state';
import { app, el, root, sound, store } from '@dotany/web/shared.js';
import { T } from './i18n.js';
import { icon, LUCIDE } from './icons.js';
import * as be from './backend.js';
import { setImageResolver } from './schema.js';
import { baseName, dirOf, isAbsolute, isRemotePath, joinPath, onSameShare } from '@dotany/web/paths.js';
import { parseMarkdown, serializeMarkdown, parseText, serializeText } from './markdown.js';
import { blocksToDoc, docToBlocks, assignListNumbers } from './docx-view.js';
import { view, showDoc, motion, setPaged, resetBar } from './doc-editor.js';
import { createArtView } from './art-view.js';
import {
  shell,
  appEl,
  chrome,
  dot,
  findBtn,
  followBtn,
  filterBtn,
  fmtBtn,
  wrapBtn,
  treeBtn,
  openPop,
  closePop,
  pageBtn,
  scroller,
} from './shell.js';
import { modKey } from '@dotany/web/keys.js';

// a window keeps one document: this is what it knows about it
export const cur = {
  kind: '',
  source: null,
  path: null,
  name: '',
  dir: null,
  encoding: 'UTF-8',
  eol: 'LF',
  pickedEncoding: null,
  pickedEol: null,
  lossy: false,
  size: 0,
  long: false,
  large: false,
  lang: null,
  sources: null,
};
const DOC_KINDS = ['doc', 'md', 'txt'];
const isArticle = () => DOC_KINDS.includes(cur.kind);
// DOS text art (.nfo, .diz, .ans): shown in a view of its own, never edited (art-view.js)
const ART_KINDS = ['nfo', 'ans'];
const isArt = () => ART_KINDS.includes(cur.kind);
const art = createArtView({ el, parent: appEl, before: chrome });
app.artText = () => art.text();
// a very long note is smoother in the code view, which only ever draws the lines on screen
const ARTICLE_MAX_CHARS = 400_000,
  ARTICLE_MAX_LINES = 8000;
const SIZE_LIMIT = 20 * 1024 * 1024;

// ---------- which view a file belongs in ----------
const LANGS = {
  json: ['json', 'JSON'],
  jsonc: ['json', 'JSON'],
  py: ['python', 'Python'],
  pyw: ['python', 'Python'],
  js: ['js', 'JavaScript'],
  mjs: ['js', 'JavaScript'],
  cjs: ['js', 'JavaScript'],
  jsx: ['jsx', 'JavaScript'],
  ts: ['ts', 'TypeScript'],
  mts: ['ts', 'TypeScript'],
  cts: ['ts', 'TypeScript'],
  tsx: ['tsx', 'TypeScript'],
  html: ['html', 'HTML'],
  htm: ['html', 'HTML'],
  css: ['css', 'CSS'],
  yaml: ['yaml', 'YAML'],
  yml: ['yaml', 'YAML'],
  xml: ['xml', 'XML'],
  svg: ['xml', 'XML'],
  toml: [null, 'TOML'],
  ini: [null, 'INI'],
  srt: [null, 'SRT'],
  vtt: [null, 'VTT'],
  md: [null, 'Markdown'],
  markdown: [null, 'Markdown'],
  txt: [null, null],
};
const extOf = (name) => (name.match(/\.([^.]+)$/) || [, ''])[1].toLowerCase();
function codeKind(name) {
  const [id, label] = LANGS[extOf(name)] || [null, null];
  const kind = id === 'json' ? 'json' : id === 'python' ? 'python' : 'plain';
  return { kind, langId: id, label };
}

// ---------- the code view arrives when it is first needed ----------
let code = null,
  codeLoading = null;
const buttons = () => ({ find: findBtn, wrap: wrapBtn, tree: treeBtn, follow: followBtn, filter: filterBtn });
export function ensureCode() {
  if (code) return Promise.resolve(code);
  if (!codeLoading) {
    codeLoading = import('./code-mode.js').then(({ createCode }) => {
      code = createCode({
        el,
        T,
        sound,
        parent: appEl,
        before: chrome,
        buttons: buttons(),
        makeBtn: null,
        openPop,
        closePop,
        onEdit: () => noteEdit(),
        saveByHand: () => saveByHand(),
        openSaveAs: () => app.openSaveAs(app.saveAsBtn),
        escape: () => app.closeSettings() || app.closeSaveAs(),
        pickEol: (v) => {
          cur.pickedEol = v;
          noteEdit();
        },
        pickEncoding: (v) => {
          cur.pickedEncoding = v;
          noteEdit();
        },
      });
      app.code = code;
      app.applyLang();
      return code;
    });
  }
  return codeLoading;
}

// ---------- the status line: the family's state and dot (shell.js); Text says when ----------
let edited = false,
  editVersion = 0,
  saving = null;
// a finished save says so, then goes quiet, as the family's line does; unsaved work stays said
export const setState = (k, arg = '') => shell.setState(k, arg);
const clearState = () => shell.setState('');
// Save beside the state, while there's something to save by hand
export const syncSaveBtn = () => shell.saveReady(edited && (!app.autoSave || !cur.path));
app.syncSaveBtn = syncSaveBtn;
// auto-save is the family's setting (dotany-autosave), off by default: nothing reaches someone's file until they save it
Object.defineProperty(app, 'autoSave', { get: () => store.get('dotany-autosave') === 'on', configurable: true });
export const isEdited = () => edited;
app.isEdited = isEdited;
// the family's dot goes orange at once and dark after a moment, or the file saves itself then (shell.changed)
export function noteEdit() {
  if (app.switching) return;
  edited = true;
  editVersion++;
  shell.changed();
  syncSaveBtn();
}
app.noteEdit = noteEdit;
app.autoSaveNow = () => {
  if (!app.autoSave || !cur.path) return false;
  save({ quiet: true });
  return true;
};
const markClean = () => {
  edited = false;
  shell.changed();
  syncSaveBtn();
};
app.markClean = markClean;

export function currentText() {
  if (cur.kind === 'doc' || isArt()) return '';
  if (cur.kind === 'md') return serializeMarkdown(view.state.doc, cur.sources);
  if (cur.kind === 'txt') return serializeText(view.state.doc);
  return code ? code.text() : '';
}
app.currentText = currentText;

const saveByHand = () => {
  if (edited || !cur.path) sound();
  return save();
};
app.saveByHand = saveByHand;

// the words that ask for a decision or report a failure keep the edits; only a real write marks the work as saved
function afterSave(r, version, opts = {}) {
  if (r.status === 'written' || r.status === 'unchanged') {
    cur.path = r.path;
    cur.name = r.name;
    cur.dir = dirOf(r.path);
    shell.named({ path: cur.path, name: cur.name });
    // lists made here keep the numbers the file gave them
    const tr = assignListNumbers(view.state, r.assigned);
    if (tr) {
      app.switching = true;
      view.dispatch(tr);
      app.switching = false;
    }
    if (version === editVersion) {
      markClean();
      setState('已保存');
    } else {
      dot.className = 'dot edited';
      if (app.autoSave) noteEdit();
    }
    if (shell.noticeKind === 'savefail') hideNotice();
    shell.savedInPlace();
    return true;
  }
  if (r.status === 'saved' || r.status === 'nochange') {
    cur.path = r.path;
    cur.name = r.name;
    cur.dir = dirOf(r.path);
    shell.named({ path: cur.path, name: cur.name });
    cur.encoding = r.encoding;
    if (r.eol) cur.eol = r.eol;
    cur.pickedEncoding = null;
    cur.pickedEol = null;
    if (code && !isArticle()) {
      code.setEncoding(cur.encoding);
      code.setEol(cur.eol);
      code.refresh();
    }
    if (version === editVersion) {
      markClean();
      setState('已保存');
    } else {
      dot.className = 'dot edited';
      if (app.autoSave) noteEdit();
    }
    if (shell.noticeKind === 'savefail') hideNotice();
    shell.savedInPlace();
    return true;
  }
  if (r.status === 'unmappable') {
    if (!opts.quiet || !unmappableShown) askEncoding(r);
    return false;
  }
  if (r.status === 'conflict') {
    showNotice(r.deleted ? 'deleted' : 'changed');
    return false;
  }
  if (r.status === 'failed') {
    showSaveFail(r.kind);
    dot.className = 'dot edited';
    return false;
  }
  return false;
}
/** True once what is on screen is safely on disk. A window with no file yet asks where to put it. */
export async function save(opts = {}) {
  if (!cur.path) {
    if (cur.kind) app.openSaveAs();
    return false;
  }
  if (!edited) return true;
  if (saving) await saving;
  if (!edited) return true;
  const version = editVersion;
  const job =
    cur.kind === 'doc'
      ? be.saveWord(docToBlocks(view.state.doc))
      : be.saveDocument(currentText(), cur.pickedEncoding, cur.pickedEol);
  saving = job
    .catch(() => {})
    .then(() => {
      saving = null;
    });
  try {
    return afterSave(await job, version, opts);
  } catch (e) {
    showSaveFail('other');
    return false;
  }
}
// turning auto-save on saves a named file at once; a file with no name yet waits for its first save
app.save = () => (cur.path ? save() : Promise.resolve(false));
export const settleSaveAs = (r, version) => afterSave(r, version);
export const editStamp = () => editVersion;

// ---------- this character can't be written in this encoding ----------
let unmappableShown = false;
// the family's close prompt asks it: Save with UTF-8, then what it was asked for goes on
function askEncoding(r) {
  unmappableShown = true;
  shell.askEncoding(
    [
      el('span', 'ask-name', T('{encoding} 无法保存 ', { encoding: r.encoding })),
      el('span', '', r.chars.slice(0, 6).join(' ') + (r.chars.length > 6 ? ' …' : '')),
    ],
    async () => {
      cur.pickedEncoding = 'UTF-8';
      return shell.cardOpen ? shell.retrySaveAs() : save();
    }
  );
}

// ---------- what a text file turns into on screen ----------
function resetChrome() {
  // the page shows before Text puts the caret in it (the family's empty window would still hide it)
  root.classList.remove('is-empty');
  hideNotice();
  setLocked(false);
  if (pageBtn.getAttribute('aria-pressed') === 'true') setPaged(false);
  app.closeSaveAs(true);
  app.docFind.close();
  if (code) {
    code.closeTransient();
    code.leave();
  }
  markClean();
  clearState();
  unmappableShown = false;
  art.clear();
  cur.readOnly = false;
  cur.source = null;
}
// a new file not saved yet keeps the name the program gave it in the language on screen: 未命名.md, Untitled.md
export function renameUntitled() {
  const m = !cur.path && /^(?:未命名|Untitled)(\.\w+)$/.exec(cur.name || '');
  if (!m) return;
  cur.name = T('未命名') + m[1];
}
function setRootKind() {
  const article = isArticle(),
    artFile = isArt();
  root.classList.toggle('code', !article && !artFile);
  root.classList.toggle('md-file', cur.kind === 'md');
  root.classList.toggle('txt-file', cur.kind === 'txt');
  root.classList.toggle('art-file', artFile);
  root.classList.toggle('ans-file', cur.kind === 'ans');
  if (article || artFile) root.classList.remove('json-file', 'log-file', 'tree-on');
}
app.setKind = (k) => {
  cur.kind = k;
  app.kind = k;
};
function imagesFrom(doc) {
  const paths = [];
  doc.descendants((n) => {
    const path = n.type.name === 'image' && !n.attrs.src.startsWith('data:') && localImage(n.attrs.src);
    if (path) paths.push(path);
    return true;
  });
  return paths;
}
// a picture's path; none for one on another computer or a device, which then shows as a missing one, unless it lies on
// the same network share as the document itself (the backend's `allow_images` holds to the same rule)
function localImage(src) {
  // %20, %28, %23… as Word to Markdown writes a picture's name
  let s = src;
  try {
    s = decodeURIComponent(src);
  } catch {
    /* the address is what it is */
  }
  const path = isAbsolute(s) ? s : joinPath(cur.dir, s);
  return isRemotePath(path) && !onSameShare(path, cur.path) ? null : path;
}
setImageResolver((src) =>
  src.startsWith('data:')
    ? src
    : src.startsWith('docx:')
      ? be.fileSrc(be.windowLabel + '/' + src.slice(5), 'dotmedia')
      : be.fileSrc(localImage(src) ?? '')
);

// a Word file: the words become the editor's document; what can't be edited stays in it, locked.
// doc, rtf and odt come the same way, read into a Word document, and stay read-only: a change is saved as Word
function presentWord(r) {
  resetChrome();
  cur.path = r.path || null;
  cur.dir = dirOf(r.path);
  cur.size = r.size || 0;
  cur.long = false;
  cur.large = false;
  cur.sources = null;
  cur.lang = null;
  cur.name = r.name;
  cur.readOnly = !!r.read_only;
  cur.source = r.source || null;
  app.setKind('doc');
  setRootKind();
  app.refreshSaveFormats();
  showDoc(blocksToDoc(r.blocks, T));
  view.focus();
  if (cur.readOnly) {
    setLocked(true);
    showNotice(cur.source ? (r.has_locked ? 'formatPartial' : 'formatReadOnly') : 'wordReadOnly');
  } else if (r.has_locked) showNotice('wordPartial');
}
// DOS text art: the characters (or an .ans's picture) in their DOS face, view-only; Save As writes them as plain text
async function presentArt(r) {
  resetChrome();
  Object.assign(cur, {
    path: r.path || null,
    name: r.name,
    dir: dirOf(r.path),
    size: r.size || 0,
    long: false,
    large: false,
    sources: null,
    lang: null,
    readOnly: true,
    source: null,
  });
  app.setKind(r.kind === 'ans' ? 'ans' : 'nfo');
  setRootKind();
  await art.show(r.text, r.kind);
  setLocked(true);
  showNotice('artReadOnly');
  art.focus();
}
async function present(r) {
  if (r.status === 'docx') {
    presentWord(r);
    return;
  }
  if (r.status === 'art') {
    await presentArt(r);
    return;
  }
  cur.path = r.path || null;
  cur.name = r.name;
  cur.dir = dirOf(r.path);
  cur.size = r.size || 0;
  cur.encoding = r.encoding;
  cur.eol = r.eol;
  cur.lossy = !!r.lossy;
  cur.pickedEncoding = null;
  cur.pickedEol = null;
  cur.long = r.status === 'long';
  cur.large = !!r.large;
  cur.sources = null;
  cur.lang = null;
  const ext = extOf(r.name);
  let kind,
    parsed = null,
    plainDoc = null,
    info = codeKind(r.name);
  if (cur.long) kind = 'log';
  else {
    const small =
      r.text.length <= ARTICLE_MAX_CHARS && r.text.split('\n', ARTICLE_MAX_LINES + 1).length <= ARTICLE_MAX_LINES;
    if ((ext === 'md' || ext === 'markdown') && small) {
      parsed = parseMarkdown(r.text);
      if (parsed) kind = 'md';
    }
    if (ext === 'txt' && small) {
      plainDoc = parseText(r.text);
      kind = 'txt';
    }
    if (!kind) kind = info.kind;
  }
  resetChrome();
  app.setKind(kind);
  setRootKind();
  app.refreshSaveFormats();
  if (kind === 'md' || kind === 'txt') {
    if (parsed) {
      cur.sources = parsed.sources;
      await be.allowImages(imagesFrom(parsed.doc));
      showDoc(parsed.doc);
    } else showDoc(plainDoc);
    view.focus();
  } else {
    const c = await ensureCode();
    c.setEncoding(cur.encoding);
    c.setEol(cur.eol);
    if (kind === 'log') {
      await c.openLong({ logLabel: ext === 'log' });
    } else {
      cur.lang = info.label || (ext === 'txt' ? null : null);
      c.setLang(cur.lang);
      c.open(kind, r.text, info.langId);
    }
  }
  if (cur.long && cur.large) showNotice('large');
  else if (r.certain === false) {
    shownEnc = cur.encoding;
    showNotice('encoding');
  }
}
// opening goes through the family's (shell.js): the recent list, the menu, the open dialog, a drop and the command line
// all come here, and a file that can't open is said in the family's toast
const CANT = { binary: 'refused', damaged: 'damaged', missing: 'missing', failed: 'missing' };
app.openForShell = async (path) => {
  const r = await be.openPath(path);
  if (r.status === 'text' || r.status === 'long' || r.status === 'docx' || r.status === 'art') {
    sound();
    await present(r);
    return { status: 'ok', path: cur.path, name: cur.name, dir: cur.dir };
  }
  if (r.status === 'missing') be.forgetRecent(path);
  return { status: CANT[r.status] || 'damaged' };
};
export const openPath = (path) => shell.openPath(path);
export const openRequest = (path) => shell.openRequest(path);

// ---------- new: in this window when it is empty, otherwise in a new window ----------
export async function createInHere(f) {
  if (f.kind === 'doc') {
    const o = await be.newWordDocument();
    presentWord({ ...o, name: T('未命名') + f.ext });
    shell.opened({ status: 'ok', path: null, name: cur.name, dir: null });
    return;
  }
  resetChrome();
  await be.newDocument();
  const name = T('未命名') + f.ext;
  Object.assign(cur, {
    path: null,
    name,
    dir: null,
    encoding: 'UTF-8',
    eol: 'LF',
    pickedEncoding: null,
    pickedEol: null,
    lossy: false,
    long: false,
    large: false,
    sources: null,
    lang: f.kind === 'plain' ? f.label : null,
  });
  app.setKind(f.kind);
  setRootKind();
  app.refreshSaveFormats(f);
  shell.opened({ status: 'ok', path: null, name, dir: null });
  if (f.kind === 'md') {
    const p = parseMarkdown('');
    cur.sources = p.sources;
    showDoc(p.doc);
    view.focus();
    return;
  }
  if (f.kind === 'txt') {
    showDoc(parseText(''));
    view.focus();
    return;
  }
  const c = await ensureCode();
  c.setEncoding('UTF-8');
  c.setEol('LF');
  c.setLang(f.kind === 'plain' ? f.label : null);
  c.open(f.kind, f.blank || '', f.langId || null);
  // a template's own words are not an edit, but they must be written when saved
  if (f.blank) {
    /* saved with the rest on the first save */
  }
}
// New while a file is open makes it in a new window of Text's; in the empty window, here
export function createFormat(f) {
  if (cur.kind) return be.newWindow(f.id, null);
  return createInHere(f);
}
app.createFormat = createFormat;

// ---------- the file menu, the empty window, the drop sheet and the close prompt are the family's (shell.js) ----------
// leaving the file: the family's empty window, after Text has put its own parts away
app.clearForShell = () => {
  resetChrome();
  Object.assign(cur, { path: null, name: '', dir: null, long: false, large: false, sources: null, readOnly: false });
  app.setKind('');
  root.classList.remove('code', 'md-file', 'txt-file', 'json-file', 'log-file', 'tree-on', 'art-file', 'ans-file');
  if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
};
export const goEmpty = () => shell.goEmpty();
// opening over unsaved work, or closing, asks first: Save, Don't Save, Cancel
export const askClose = (next) => shell.guard(next);
app.askClose = askClose;

// ---------- when something is off with the file: the family's row under the top (shell.js), the words stay readable ----
// Text's own kinds go in it: the encoding's menu, read-only Word and its relatives, a large file; the row
// pushes the words down and the caret goes with them. Changed outside, deleted and a failed save are the family's
let shownEnc = 'GBK',
  locked = false;
const ENCODINGS = ['UTF-8', 'GBK', 'Big5', 'Shift_JIS'];
function setLocked(on) {
  locked = on;
  view.setProps({ editable: () => !locked });
  if (code) code.setLocked(on);
  root.classList.toggle('locked', on);
}
const encodingText = () =>
  T('未能识别文件编码，暂以 {encoding} 预览。为防止损坏原文件，已锁定为只读模式。', { encoding: shownEnc });
// always from the original bytes, so a wrong choice costs nothing
async function chooseEncoding(label) {
  shownEnc = label;
  const r = await be.reopenAs(label);
  cur.encoding = label;
  if (!r) {
    // the long scroll reads the file again in the new encoding
    if (code) {
      code.setEncoding(label);
      await code.openLong({ logLabel: extOf(cur.name) === 'log' });
    }
    showNotice('encoding');
    return;
  }
  cur.lossy = r.lossy;
  cur.eol = r.eol;
  if (cur.kind === 'md') {
    const p = parseMarkdown(r.text);
    if (p) {
      cur.sources = p.sources;
      showDoc(p.doc);
    } else {
      // this reading doesn't fit the article view
      const info = codeKind(cur.name);
      app.setKind(info.kind);
      setRootKind();
      app.refreshSaveFormats();
      const c = await ensureCode();
      c.setLang(info.label);
      c.setEncoding(label);
      c.setEol(r.eol);
      c.open(info.kind, r.text, info.langId);
    }
  } else if (cur.kind === 'txt') showDoc(parseText(r.text));
  else if (code) {
    code.setEncoding(label);
    code.setEol(r.eol);
    code.open(cur.kind, r.text, codeKind(cur.name).langId);
  }
  markClean();
  showNotice('encoding');
}
const notices = {
  encoding: {
    icon: 'alert',
    text: encodingText,
    actions: () => [
      [
        '字符编码',
        (b) => {
          openPop(
            b,
            ENCODINGS.map((v) => ({ value: v, label: v })),
            shownEnc,
            chooseEncoding
          );
          return true;
        },
      ],
      // bytes this reading can't hold would be lost on saving, and a long scroll never edits
      ...(cur.lossy || cur.long ? [] : [['仍然编辑', () => setLocked(false)]]),
    ],
  },
  large: { icon: 'info', text: () => T('文件体积较大，已自动以只读长卷模式打开。'), actions: [['知道了', null]] },
  wordPartial: {
    icon: 'info',
    text: () => T('文档包含修订记录、批注、脚注或文本框等复杂元素。虽无法在此编辑，但保存时将原样保留。'),
    actions: [['知道了', null]],
  },
  wordReadOnly: {
    icon: 'alert',
    text: () => T('为保护原始排版数据，当前以只读模式打开。你可以另存为副本以进行编辑。'),
    actions: [
      [
        '另存为…',
        () => {
          app.openSaveAs();
        },
      ],
      ['知道了', null],
    ],
  },
  formatReadOnly: {
    icon: 'alert',
    text: () => T('当前格式仅支持阅览，如需修改请另存为 Word 文档（.docx）。'),
    actions: [
      [
        '另存为…',
        () => {
          app.openSaveAs();
        },
      ],
      ['知道了', null],
    ],
  },
  artReadOnly: {
    icon: 'alert',
    text: () => T('当前格式仅支持阅览，如需修改请另存为纯文本（.txt）。'),
    actions: [
      [
        '另存为…',
        () => {
          app.openSaveAs();
        },
      ],
      ['知道了', null],
    ],
  },
  formatPartial: {
    icon: 'alert',
    text: () =>
      T('当前格式仅支持阅览，如需修改请另存为 Word 文档。注意：脚注、文本框等复杂元素无法显示，亦不会写入新文档。'),
    actions: [
      [
        '另存为…',
        () => {
          app.openSaveAs();
        },
      ],
      ['知道了', null],
    ],
  },
};
app.noticeKinds = notices;
function showNotice(kind) {
  if (kind === 'encoding') setLocked(true);
  shell.showNotice(kind);
}
const hideNotice = () => shell.hideNotice();
const showSaveFail = (kind) => shell.saveFailed(kind);
// Keep Current Edits marks the file unsaved, as in every tool; a long scroll is never edited
app.keptMine = () => {
  if (!cur.long || shell.noticeKind === 'deleted') noteEdit();
};
app.noticeMoved = (on) => {
  root.classList.toggle('has-notice', on);
  textMoved();
};
// the row pushes the words down, and the caret goes with them
function textMoved() {
  if (motion) motion.relayout(560);
  if (code) code.nudge();
}

// ---------- printing goes through the system window; the page itself says what comes out on paper ----------
const printCode = el('pre', 'print-code');
document.body.appendChild(printCode);
addEventListener('afterprint', () => {
  printCode.textContent = '';
  root.classList.remove('print-code-on');
});
export async function printFile() {
  closePop();
  if (!cur.kind) return;
  if (isArt()) {
    printCode.textContent = art.text();
    root.classList.add('print-code-on');
  } else if (!isArticle()) {
    if (cur.long && cur.size > SIZE_LIMIT) {
      setState('文件体积过大，无法执行打印。');
      return;
    }
    printCode.textContent = cur.long ? await be.long.text() : code.text();
    root.classList.add('print-code-on');
  }
  be.printWindow();
}

// the family's keys (shell.js) open, save, print and make new files; Text's own is find, from outside its editors,
// whose own find takes it inside them
const inEditor = (t) => t.closest && t.closest('.ProseMirror, .cm-editor, .ls, input, textarea');
app.keys = (e) => {
  if (!modKey(e) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return;
  e.preventDefault();
  if (cur.kind && !inEditor(e.target)) findBtn.click();
};
findBtn.addEventListener('mousedown', (e) => e.preventDefault());
findBtn.addEventListener('click', () => {
  if (isArt()) return;
  if (!isArticle()) {
    code.actions.find();
    return;
  }
  sound();
  if (app.docFind.isOpen()) app.docFind.close();
  else app.docFind.open(false);
});
[followBtn, filterBtn].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
followBtn.addEventListener('click', () => {
  sound();
  code.actions.follow();
  code.focus();
});
filterBtn.addEventListener('click', () => code.actions.filterMenu(filterBtn));
fmtBtn.addEventListener('mousedown', (e) => e.preventDefault());
fmtBtn.addEventListener('click', () => code.actions.formatMenu(fmtBtn));
wrapBtn.addEventListener('mousedown', (e) => e.preventDefault());
wrapBtn.addEventListener('click', () => {
  sound();
  code.actions.wrap();
  code.focus();
});
treeBtn.addEventListener('mousedown', (e) => e.preventDefault());
treeBtn.addEventListener('click', () => {
  sound();
  code.actions.tree();
  code.focus();
});

// a click in the margins lands on the nearest line
scroller.addEventListener('mousedown', (e) => {
  if (e.button !== 0 || view.dom.contains(e.target) || e.target.closest('button, .bub')) return;
  const box = view.dom.getBoundingClientRect();
  const found = view.posAtCoords({
    left: Math.max(box.left + 1, Math.min(box.right - 1, e.clientX)),
    top: Math.max(box.top + 1, Math.min(box.bottom - 1, e.clientY)),
  });
  e.preventDefault();
  if (found) view.dispatch(view.state.tr.setSelection(TextSelection.near(view.state.doc.resolve(found.pos))));
  view.focus();
});

app.focus = () => {
  if (shell.askOpen || !cur.kind || root.classList.contains('new-open')) return;
  if (isArticle()) view.focus();
  else if (isArt()) art.focus();
  else if (code) code.focus();
};
export const isCodeLoaded = () => !!code;
export const refreshLang = () => {
  if (isArticle()) resetBar();
};
