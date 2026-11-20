// Text's window: the family's (shared/web/shell.js, createShell) with Text's own parts in it — its buttons after the
// separator, the format bar under the top line, the page in its scroller, and the small menus its bars open.
// What the family's parts do for Text is asked of the other modules through `app`, once they're there.
import { createShell } from '@dotany/web/shell.js';
import { app, el, sound } from '@dotany/web/shared.js';
import { ICON, LUCIDE as CODE_ICONS } from './icons.js';
import { T } from './i18n.js';
import { NEW_GROUPS, FORMAT } from './formats.js';

// the family's parts draw with Text's own icons, so they look like the rest of Text
const inner = (svg) => svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, '');
const ICONS = {
  ...CODE_ICONS,
  settings: inner(ICON.settings),
  saveAs: inner(ICON.saveAs),
  pages: inner(ICON.pages),
  chevron: '<path d="m6 9 6 6 6-6"/>',
  back: '<path d="m15 18-6-6 6-6"/>',
};

export const shell = createShell({
  name: 'DotAny Text',
  icons: ICONS,
  notices: () => import('./notices.generated.js'),
  soundNote: '触发按钮操作或完成保存时播放提示音，打字输入过程保持静默。',
  licences: [
    ['JetBrains Mono', '代码字体', 'SIL OFL 1.1', 'font'],
    ['Unscii', '字符画字体', 'Public Domain', 'font'],
    ['ProseMirror', '编辑器内核', 'MIT', 'core'],
    ['CodeMirror', '代码编辑内核', 'MIT', 'core'],
  ],
  count: true,
  print: () => app.printFile(),
  multiWindow: true,
  formats: FORMAT,
  formatsFor: () => app.formatsFor(),
  formatOf: () => app.formatsFor()[0],
  saveNote: (k) => FORMAT[k].note,
  noteAtFoot: true,
  askFirst: true,
  save: (path, key) => app.saveTo(path, key, shell),
  saved: (r) => (r.said ? 'said' : null),
  beforeSaveAs: () => closePop(),
  noticeKinds: () => app.noticeKinds,
  noticeMoved: (on) => app.noticeMoved(on),
  keptMine: () => app.keptMine(),
  saveInPlace: { save: () => app.saveByHand(), auto: () => app.autoSaveNow() },
  newFile: { label: '新建文档', menuLabel: '新建…', groups: NEW_GROUPS, create: (f) => app.createFormat(f) },
  // Text decides on opening, from what's in the file
  canOpen: () => 'ok',
  open: (path) => app.openForShell(path),
  clear: () => app.clearForShell(),
  dirty: () => app.isEdited(),
  discard: () => app.markClean(),
  beforeGuard: () => {
    closePop();
    app.closeSettings();
    app.closeSaveAs(true);
  },
  keys: (e) => app.keys(e),
  focus: () => app.focus && app.focus(),
  editors: '.ProseMirror, .cm-editor, .ls',
  textMenus: '.ProseMirror, .cm-content, .ls-rows',
  beforeShow: () => app.beforeShow(),
  startup: () => app.startup(),
});
const { chrome, head, dot, count: countEl } = shell;
const appEl = shell.app;
countEl.classList.add('doc-only');

// Text's own buttons, after the separator: the page view, find, and the code view's
const headBtn = (id, iconName, label, tip, ...cls) => {
  const b = shell.headButton(id, iconName, label, tip);
  b.classList.add(...cls);
  return b;
};
const pageBtn = headBtn('pageToggle', 'pages', '页面视图', '页面视图', 'doc-only', 'word-only');
pageBtn.setAttribute('aria-pressed', 'false');
const findBtn = headBtn('findBtn', 'search', '查找', '查找 · {Mod+F}');
findBtn.setAttribute('aria-pressed', 'false');
const fmtBtn = headBtn('fmtBtn', 'format', '格式化', '格式化 · {Shift+Alt+F}', 'code-only', 'json-only');
const followBtn = headBtn('followBtn', 'follow', '实时跟随', '自动滚动至文件末尾的最新内容', 'code-only', 'log-only');
followBtn.setAttribute('aria-pressed', 'false');
const filterBtn = headBtn('filterBtn', 'filter', '筛选', '筛选', 'code-only', 'log-only');
filterBtn.setAttribute('aria-pressed', 'false');
const wrapBtn = headBtn('wrapBtn', 'wrap', '自动换行', '自动换行 · {Alt+Z}', 'code-only');
wrapBtn.setAttribute('aria-pressed', 'false');
const treeBtn = headBtn('treeBtn', 'tree', '大纲结构', '大纲结构', 'code-only', 'json-only');
treeBtn.setAttribute('aria-pressed', 'false');
// the separator the shell puts before Text's own buttons: it goes with them where none of them shows (text art)
pageBtn.previousElementSibling.classList.add('tool-sep');
const saveAsBtn = document.getElementById('saveAs'),
  setBtn = document.getElementById('settings'),
  fileBtn = document.getElementById('fileMenu');

// the format bar, under the top line and above the notices
const bar = el('div', 'bar');
bar.setAttribute('role', 'toolbar');
bar.setAttribute('aria-label', '格式工具栏');
const seg = el('div', 'seg'),
  pill = el('i', 'pl');
seg.appendChild(pill);
head.after(bar);
// the small menus Text's bars open (the file menu is the family's)
const pop = el('div', 'pop');
pop.setAttribute('role', 'menu');
document.body.appendChild(pop);
// the page: a scroller over the whole window, the words going under the glass at the top
const scroller = el('main', 'scroll');
const stage = el('div', 'stage');
const editorHost = el('div');
const sels = el('div', 'sels');
const lastNo = el('div', 'last-no');
stage.append(sels, editorHost, lastNo);
scroller.appendChild(stage);
shell.stage.replaceWith(scroller);
shell.translates(bar);

function makeBtn(cls, html, label, tip, run) {
  const b = el('button', cls);
  b.type = 'button';
  b.innerHTML = html;
  b.setAttribute('aria-label', label);
  if (tip) b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    run(b);
    if (!pop.classList.contains('on')) app.focus();
  });
  return b;
}

// ---------- Text's own menus: centred under the button that opens them, or above it where there's no room ----------
let popFor = null;
function closePop() {
  if (!popFor) return false;
  pop.classList.remove('on');
  popFor = null;
  return true;
}
function openPop(btn, items, current, pick) {
  if (popFor === btn) {
    closePop();
    app.focus();
    return;
  }
  pop.textContent = '';
  items.forEach((it) => {
    if (it.sep) {
      pop.appendChild(el('i', 'pop-sep'));
      return;
    }
    if (it.title) {
      pop.appendChild(el('span', 'pop-title', T(it.title)));
      return;
    }
    const b = el('button');
    b.type = 'button';
    b.setAttribute('role', 'menuitem');
    if (it.swatch) {
      const s = el('i', 'sw');
      s.style.background = it.swatch;
      b.appendChild(s);
    }
    if (it.icon) b.insertAdjacentHTML('beforeend', it.icon);
    b.appendChild(el('span', '', T(it.label)));
    if (it.hint) b.appendChild(el('span', 'hint', T(it.hint)));
    if (String(it.value) === String(current)) b.setAttribute('aria-pressed', 'true');
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      closePop();
      pick(it.value);
      if (!app.dlgOpen()) app.focus();
    });
    pop.appendChild(b);
  });
  const r = btn.getBoundingClientRect();
  pop.classList.remove('on');
  pop.style.left = '0px';
  pop.style.top = '0px';
  const w = pop.offsetWidth;
  pop.style.left = Math.max(8, Math.min(innerWidth - w - 8, r.left + r.width / 2 - w / 2)) + 'px';
  const up = r.bottom + pop.offsetHeight + 16 > innerHeight;
  pop.style.top = (up ? r.top - pop.offsetHeight - 8 : r.bottom + 8) + 'px';
  pop.style.transformOrigin = up ? '50% 100%' : '50% 0';
  popFor = btn;
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (popFor === btn) pop.classList.add('on');
    })
  );
}
document.addEventListener('pointerdown', (e) => {
  if (!popFor || pop.contains(e.target) || (popFor && popFor.contains(e.target))) return;
  closePop();
});
export const popOpenFor = () => popFor;

export {
  chrome,
  head,
  bar,
  seg,
  pill,
  pop,
  stage,
  scroller,
  editorHost,
  sels,
  lastNo,
  dot,
  fileBtn,
  countEl,
  pageBtn,
  saveAsBtn,
  setBtn,
  findBtn,
  followBtn,
  filterBtn,
  fmtBtn,
  wrapBtn,
  treeBtn,
  appEl,
  makeBtn,
  openPop,
  closePop,
};
