// DotAny Sheets: the family's window around a table. The table itself — hairlines across, one frame that glides from
// cell to cell, the dock with the cell's contents or the selection's sum, filter and sort that only change what you see,
// find and replace, the sheets as a sliding capsule beside the file's name — is drawn and edited here. Everything around
// it, from the top bar to settings and save as, is shared/web. Reading and writing files is xlsx.js, sheet.js and the
// Rust side.
import { createShell } from '@dotany/web/shell.js';
import { el, root, reduce, sound, icon, EASE } from '@dotany/web/shared.js';
import { T, num, size as sizeText } from '@dotany/web/i18n.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';
import {
  sniffDelimiter,
  parseDelimited,
  lineEndOf,
  writeDelimited,
  colName,
  formatNumber,
  looksNumber,
  evaluate,
  parse,
  NotSupported,
  FormulaError,
  moveRefs,
  shiftFormula,
  tidyFormula,
  epoch,
} from './sheet.js';
import { readXlsx, patchXlsx, freshXlsx } from './xlsx.js';
import { modKey, yRedoes } from '@dotany/web/keys.js';

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
const FORMATS = {
  xlsx: { ext: '.xlsx', label: 'XLSX' },
  csv: { ext: '.csv', label: 'CSV' },
  tsv: { ext: '.tsv', label: 'TSV' },
};
const BOOKISH = ['xlsx', 'xlsm', 'xls', 'ods', 'xlsb', 'fods', 'dbf', 'sqlite', 'new'];
// read on the Rust side into cells (dotany-table), and only ever viewed
const MODELLED = ['xls', 'ods', 'xlsb', 'fods', 'dbf', 'sqlite'];
const FORMAT_NAMES = { new: 'XLSX', sqlite: 'SQLite' };

const shell = createShell({
  name: 'DotAny Sheets',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽表格至此',
  fileKind: '电子表格',
  soundNote: '触发按钮操作时播放提示音。',
  licences: [
    ['fflate', 'XLSX', 'MIT'],
    ['calamine', 'XLS、ODS', 'MIT'],
    ['SQLite', 'SQLite', 'Public Domain'],
    ['chardetng', '字符编码', 'MIT / Apache-2.0'],
  ],
  formats: FORMATS,
  formatOf: () => (!readOnly && (kind === 'csv' || kind === 'tsv') ? kind : 'xlsx'),
  cantOpen: { damaged: '文件已损坏，或并非有效的电子表格。', locked: '暂不支持打开受密码保护的加密表格。' },
  failWords: {
    encoding: '表格中包含当前字符编码无法表示的文字，未能保存。请尝试切换为 UTF-8，当前编辑仍完整保留在此窗口中。',
  },
  open: openPath,
  show(r) {
    if (pending && pending.path === (r.path || NEW)) holdPending(r);
  },
  clear: closeBook,
  dirty: () => opened && hAt !== savedAt,
  discard() {
    savedAt = hAt;
  },
  estimate,
  save: saveTo,
  saved: afterSave,
  saveRows,
  saveNote,
  keys,
  newFile: { label: '新建表格', run: () => makeBook() },
});
const { stage: shellStage, head, metaEl } = shell;

const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const MOVE = { duration: 240, easing: EASE };
const money = (b) => sizeText(b, { zero: true, gb: true });
const plain = (n) =>
  Math.abs(n) >= 1000 ? n.toLocaleString('en-US', { maximumFractionDigits: 2 }) : String(+n.toFixed(2));

// ---------- what's open ----------
const RH = 30,
  LH = 28,
  NO = 60,
  MINW = 40,
  DEFW = 104;
// `kind` is what the file in the window is now; `origin` what it was opened as, which is what every save starts from
let opened = false,
  fileSize = 0,
  kind = 'xlsx',
  origin = 'xlsx',
  readOnly = false,
  csvInfo = null,
  extras = null,
  pkg = null,
  names = {},
  nameList = [],
  book = [],
  at = 0,
  pending = null;
const sheet = () => book[at];
const isBook = () => BOOKISH.includes(kind);
const makeSheet = (name, rows, widths = {}, x = null, raw = null) => {
  const s = {
    name,
    rows,
    widths,
    x,
    raw,
    headerOn: false,
    sort: null,
    filters: {},
    view: null,
    sel: { r: 0, c: 0, r2: 0, c2: 0 },
  };
  s.headerOn = looksHeader(rows);
  return s;
};
function looksHeader(rows) {
  const a = rows[0],
    b = rows[1];
  if (!a || !b || a.length < 2) return false;
  const words = a.filter((x) => typeof x === 'string' && x && !looksNumber(x)).length;
  const others = (b || []).filter((x) => typeof x !== 'string' || looksNumber(x) || /\d/.test(x)).length;
  return words >= a.filter((x) => x !== '' && x != null).length && (others > 0 || rows.length > 2);
}
const colCount = (s) =>
  Math.max(
    s.rows.reduce((m, r) => Math.max(m, r ? r.length : 0), 0),
    0
  );
const shownCols = (s) => Math.max(colCount(s) + 4, 12);

// ---------- values ----------
let cache = new Map();
function cellRaw(s, r, c) {
  const row = s.rows[r];
  return row ? row[c] : undefined;
}
const sheetNamed = (name) => {
  const n = String(name).toLowerCase();
  return book.find((b) => b.name.toLowerCase() === n);
};
const treeCache = new Map();
const treeOf = (f) => {
  let t = treeCache.get(f);
  if (t === undefined) {
    try {
      t = parse(f);
    } catch (e) {
      t = e;
    }
    if (treeCache.size > 20000) treeCache.clear();
    treeCache.set(f, t);
  }
  if (t instanceof Error) throw t;
  return t;
};
function valueOf(s, r, c, stack = new Set()) {
  const x = cellRaw(s, r, c);
  if (x == null || x === '') return '';
  if (typeof x === 'string') return looksNumber(x) ? +x : x;
  if (x.f == null) return x.n;
  const key = s.name + '!' + r + ':' + c;
  if (cache.has(key)) {
    const v = cache.get(key);
    if (v instanceof Error) throw v;
    return v;
  }
  if (stack.has(key)) throw new FormulaError('#REF!');
  stack.add(key);
  try {
    const read = (name, rr, cc) => {
      const target = name ? sheetNamed(name) : s;
      if (!target) throw new FormulaError('#REF!');
      return valueOf(target, rr, cc, stack);
    };
    const v = evaluate(treeOf(x.f), { read, name: (n) => names[n.toUpperCase()] || null });
    cache.set(key, v);
    return v;
  } catch (e) {
    // a formula this code can't work out keeps the result the file came with
    if (e instanceof NotSupported && x.c != null && x.c !== '') {
      const v = typeof x.c === 'object' ? x.c.n : x.c;
      cache.set(key, v);
      return v;
    }
    const err = e instanceof NotSupported || e instanceof FormulaError ? e : new FormulaError();
    cache.set(key, err);
    throw err;
  } finally {
    stack.delete(key);
  }
}
const BAD = '\u0000bad';
function textOf(s, r, c) {
  const x = cellRaw(s, r, c);
  if (x == null || x === '') return '';
  if (typeof x === 'string') return x;
  if (x.f == null) return formatNumber(x.n, x.fmt);
  try {
    const v = valueOf(s, r, c);
    return typeof v === 'number' ? formatNumber(v, x.fmt) : typeof v === 'boolean' ? (v ? 'TRUE' : 'FALSE') : String(v);
  } catch {
    return BAD;
  }
}
const shownText = (t) => (t === BAD ? T('#错误') : t);
// what a file gets: the shown text, with an error as spreadsheets name it
function fileText(s, r, c) {
  const t = textOf(s, r, c);
  if (t !== BAD) return t;
  try {
    valueOf(s, r, c);
  } catch (e) {
    return e instanceof FormulaError ? e.code : '#VALUE!';
  }
  return '';
}
const sourceOf = (x) =>
  x == null || x === ''
    ? ''
    : typeof x === 'string'
      ? x
      : x.f != null
        ? x.f
        : formatNumber(x.n, x.fmt === ',' ? '' : x.fmt);
// what someone typed: a formula, a number in a numbered cell of a workbook, or the words exactly as typed
const fromTyped = (t, old) => {
  const fmt = old && typeof old === 'object' ? old.fmt || '' : '';
  if (t.startsWith('=') && t.length > 1) return { f: tidyFormula(t), fmt };
  if (isBook() && fmt && looksNumber(t)) return { n: +t, fmt };
  return t;
};
const alignRight = (s, r, c) => {
  const x = cellRaw(s, r, c);
  if (x == null || x === '') return false;
  if (typeof x === 'string') return looksNumber(x);
  if (x.f == null) return true;
  try {
    return typeof valueOf(s, r, c) === 'number';
  } catch {
    return false;
  }
};

// ---------- the view: filtering and sorting only change what you see, never the file ----------
function dataRows(s) {
  if (s.view) return s.view;
  const first = s.headerOn ? 1 : 0,
    active = Object.keys(s.filters).length > 0;
  let out = [];
  for (let r = first; r < s.rows.length; r++) {
    if (active) {
      let keep = true;
      for (const c in s.filters)
        if (!s.filters[c].has(shownText(textOf(s, r, +c)))) {
          keep = false;
          break;
        }
      if (!keep) continue;
    }
    out.push(r);
  }
  if (s.sort) {
    const { c, up } = s.sort,
      val = (r) => {
        try {
          return valueOf(s, r, c);
        } catch {
          return '';
        }
      };
    const keyed = out.map((r) => [val(r), r]);
    keyed.sort((a, b) => {
      const x = a[0],
        y = b[0];
      if (x === '' && y !== '') return 1;
      if (y === '' && x !== '') return -1;
      const d =
        typeof x === 'number' && typeof y === 'number'
          ? x - y
          : String(x).localeCompare(String(y), 'zh-CN', { numeric: true });
      return (up ? d : -d) || a[1] - b[1];
    });
    out = keyed.map((k) => k[1]);
  }
  s.view = out;
  return out;
}
const hasView = (s = sheet()) => !!s && (!!s.sort || Object.keys(s.filters).length > 0);
const blanks = (s) => (hasView(s) ? 0 : Math.max(40, 60 - s.rows.length));
const hdr = () => (sheet().headerOn ? 1 : 0);
const displayCount = () => {
  const s = sheet();
  return hdr() + dataRows(s).length + blanks(s);
};
// display row → row in the file
function fileRow(d) {
  const s = sheet();
  if (s.headerOn && d === 0) return 0;
  const i = d - hdr(),
    list = dataRows(s);
  return i < list.length ? list[i] : Math.max(s.rows.length, hdr()) + (i - list.length);
}
const yOf = (d) => (d - hdr()) * RH;
const widthOf = (c) => {
  const w = sheet().widths[c];
  return w != null ? clamp(w, MINW, 560) : DEFW;
};
let colXs = [];
function measureCols() {
  colXs = [NO];
  const n = shownCols(sheet());
  for (let i = 0; i < n; i++) colXs.push(colXs[i] + widthOf(i));
}

// ---------- the window: the table in the family's stage ----------
const stage = el('main', 'sh-stage'),
  gridEl = el('div', 'sh-grid'),
  headEl = el('div', 'sh-head'),
  lettersEl = el('div', 'sh-letters'),
  frozenEl = el('div', 'sh-row sh-frozen'),
  bodyEl = el('div', 'sh-body'),
  rowsEl = el('div', 'sh-rows'),
  rangeEl = el('i', 'sh-range'),
  frameEl = el('i', 'sh-frame'),
  handle = el('i', 'sh-handle');
stage.id = 'sheetStage';
gridEl.id = 'grid';
lettersEl.id = 'letters';
bodyEl.id = 'gridBody';
frameEl.id = 'selFrame';
handle.id = 'fillHandle';
headEl.append(lettersEl, frozenEl);
bodyEl.append(rowsEl, rangeEl, frameEl, handle);
gridEl.append(headEl, bodyEl);
stage.appendChild(gridEl);
shellStage.appendChild(stage);
// the sheets: the family's sliding capsule, next to the file's name
const where = head.querySelector('.where'),
  stateEl = where.querySelector('.state'),
  fileBtn = document.getElementById('fileMenu');
const tabSeg = el('div', 'seg sh-tabs'),
  tabPill = el('i', 'pl');
tabSeg.id = 'tabs';
tabSeg.hidden = true;
tabSeg.appendChild(tabPill);
where.insertBefore(tabSeg, stateEl);
const countEl = el('span', 'count dyn');
countEl.id = 'count';
metaEl.insertBefore(countEl, metaEl.firstChild);
const headerBtn = shell.headButton('headerToggle', 'headerRow', '表头设置', '将首行设为表头');
const filterSide = shell.sidePanel('filterPanel', 'filter', '筛选'),
  filterBtn = document.getElementById('filterPanelBtn');
const findBtn = shell.headButton('findBtn', 'search', '查找', '查找 · {Mod+F}');
const infoSide = shell.sidePanel('infoPanel', 'info', '信息'),
  infoBtn = document.getElementById('infoPanelBtn');
[filterSide, infoSide].forEach((p) => p.classList.add('right'));
[headerBtn, findBtn].forEach((b) => b.setAttribute('aria-pressed', 'false'));
headerBtn.addEventListener('click', () => {
  sound();
  toggleHeader();
});

const barBtn = (id, n, tip, run) => {
  const b = el('button', 'tb');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(n);
  b.setAttribute('aria-label', tip);
  b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    run();
  });
  return b;
};
const undoBtn = barBtn('undo', 'undo', '撤销 · {Mod+Z}', () => undo()),
  redoBtn = barBtn('redo', 'redo', '重做 · {Mod+Y}', () => redo());
// the dock: which cell, what's in it, and for a range what it adds up to
const refEl = el('span', 'sh-ref dyn');
refEl.id = 'cellRef';
const barWrap = el('span', 'fi sh-bar'),
  barInput = document.createElement('input');
barInput.type = 'text';
barInput.id = 'cellBar';
barInput.spellcheck = false;
barInput.autocomplete = 'off';
barInput.setAttribute('aria-label', 'cell');
barWrap.appendChild(barInput);
const statsEl = el('div', 'sh-stats dyn');
statsEl.id = 'stats';
const viewChip = el('button', 'btn soft sh-viewchip dyn');
viewChip.type = 'button';
viewChip.id = 'clearView';
viewChip.addEventListener('mousedown', (e) => e.preventDefault());
viewChip.addEventListener('click', () => {
  sound();
  clearView();
});
const pill = el('div', 'pill'),
  pillRow = el('div', 'pill-row');
const grp = el('div', 'grp');
grp.append(undoBtn, redoBtn);
pillRow.append(refEl, barWrap, statsEl, viewChip, el('i', 'sep'), grp);
pill.appendChild(pillRow);
const dockRow = el('div', 'dock-row');
dockRow.appendChild(pill);
shell.place(dockRow);
shell.translates(pill);
inkfall(barInput);

// ---------- the notice that a format is only for viewing: the family's notice row ----------
const roNotice = shell.notice(
  () =>
    origin === 'xlsm'
      ? '当前格式仅支持阅览，如需修改请另存为 XLSX 工作簿。注意：原文件中的宏脚本不会运行，亦不会写入新文档。'
      : '当前格式仅支持阅览，如需修改请另存为 XLSX 工作簿。',
  [
    ['另存为…', () => shell.openSaveAs()],
    ['知道了', null],
  ],
  { id: 'readOnlyNotice' }
);
const showReadOnly = (on) => {
  if (on) roNotice.show();
  else roNotice.hide();
};
// an edit in a file that's only for viewing: the notice again, and the family's one sound
const refuse = () => {
  sound();
  showReadOnly(true);
};
// the table starts under the top, however tall the notice rows make it
// (the top's own 1px edge lies over the table, so the offset is one less than the top's height)
new ResizeObserver(() =>
  root.style.setProperty('--head-h', Math.round(shell.chrome.getBoundingClientRect().height) - 1 + 'px')
).observe(shell.chrome);

// ---------- drawing: only what's on screen, so a hundred thousand rows feel like ten ----------
let drawn = new Map(),
  drawnCols = '',
  paintRaf = 0;
function visibleCols() {
  const left = stage.scrollLeft + NO,
    right = stage.scrollLeft + stage.clientWidth;
  let a = 0,
    b = colXs.length - 2;
  while (a < b && colXs[a + 1] < left) a++;
  let e = a;
  while (e < colXs.length - 2 && colXs[e + 1] < right) e++;
  return [a, e];
}
function makeCells(rowEl, d, r, a, b) {
  const s = sheet(),
    frag = document.createDocumentFragment();
  for (let c = a; c <= b; c++) {
    const t = textOf(s, r, c),
      e = el(
        'span',
        'sh-c'
          + (alignRight(s, r, c) || (s.headerOn && r === 0 && alignRight(s, 1, c)) ? ' num' : '')
          + (t === BAD ? ' bad' : '')
      );
    e.style.left = colXs[c] + 'px';
    e.style.width = widthOf(c) + 'px';
    if (t) e.textContent = shownText(t);
    frag.appendChild(e);
  }
  rowEl.appendChild(frag);
}
function makeRow(d) {
  const r = fileRow(d),
    row = el('div', 'sh-row'),
    no = el('span', 'sh-no', String(r + 1));
  row.dataset.d = d;
  row.appendChild(no);
  const [a, b] = visibleCols();
  makeCells(row, d, r, a, b);
  row.style.transform = `translateY(${yOf(d)}px)`;
  return row;
}
function paint(full) {
  if (!opened) return;
  const s = sheet(),
    total = displayCount(),
    H = stage.clientHeight,
    top = stage.scrollTop;
  const first = clamp(Math.floor(top / RH) + hdr() - 2, hdr(), Math.max(hdr(), total - 1)),
    last = clamp(first + Math.ceil(H / RH) + 4, 0, total - 1);
  const cols = visibleCols().join();
  if (full || cols !== drawnCols) {
    rowsEl.textContent = '';
    drawn = new Map();
    drawnCols = cols;
    if (s.headerOn) {
      frozenEl.textContent = '';
      frozenEl.appendChild(el('span', 'sh-no', '1'));
      const [a, b] = visibleCols();
      makeCells(frozenEl, 0, 0, a, b);
    }
  }
  for (const [d, e] of drawn)
    if (d < first || d > last) {
      e.remove();
      drawn.delete(d);
    }
  for (let d = first; d <= last; d++)
    if (!drawn.has(d)) {
      const e = makeRow(d);
      rowsEl.appendChild(e);
      drawn.set(d, e);
    }
  markFrozen();
}
const askPaint = () => {
  if (paintRaf) return;
  const go = () => {
    if (!paintRaf) return;
    cancelAnimationFrame(paintRaf);
    clearTimeout(go.t);
    paintRaf = 0;
    paint();
  };
  paintRaf = requestAnimationFrame(go);
  go.t = setTimeout(go, 80);
};
stage.addEventListener('scroll', askPaint, { passive: true });
new ResizeObserver(askPaint).observe(stage);
function layout() {
  if (!opened) return;
  measureCols();
  const s = sheet(),
    total = displayCount(),
    w = colXs[colXs.length - 1];
  gridEl.style.width = w + 'px';
  bodyEl.style.height = (total - hdr()) * RH + 'px';
  root.classList.toggle('has-frozen', s.headerOn);
  headerBtn.setAttribute('aria-pressed', String(s.headerOn));
  drawLetters();
  paint(true);
  placeSelection(false);
}
function toggleHeader() {
  if (!opened) return;
  const s = sheet(),
    top = stage.scrollTop;
  s.headerOn = !s.headerOn;
  s.view = null;
  layout();
  stage.scrollTop = top;
  select(s.headerOn && sel.r === 0 ? 1 : sel.r, sel.c, s.headerOn && sel.r2 === 0 ? 1 : sel.r2, sel.c2, false, false);
}
function drawLetters() {
  const s = sheet(),
    n = shownCols(s);
  lettersEl.textContent = '';
  lettersEl.appendChild(el('span', 'sh-corner'));
  for (let c = 0; c < n; c++) {
    const e = el('span', 'sh-letter');
    e.dataset.c = c;
    e.style.left = colXs[c] + 'px';
    e.style.width = widthOf(c) + 'px';
    e.appendChild(el('span', '', colName(c)));
    if (s.filters[c] || (s.sort && s.sort.c === c)) e.classList.add('active');
    const more = el('button', 'sh-more');
    more.type = 'button';
    more.innerHTML = icon('chevron');
    more.setAttribute('aria-label', colName(c));
    more.dataset.more = c;
    e.appendChild(more);
    const grip = el('i', 'sh-grip');
    grip.dataset.grip = c;
    e.appendChild(grip);
    lettersEl.appendChild(e);
  }
}

// ---------- selection: one frame that glides from cell to cell ----------
let sel = { r: 0, c: 0, r2: 0, c2: 0 };
const box = () => ({
  r1: Math.min(sel.r, sel.r2),
  r2: Math.max(sel.r, sel.r2),
  c1: Math.min(sel.c, sel.c2),
  c2: Math.max(sel.c, sel.c2),
});
const isOne = () => sel.r === sel.r2 && sel.c === sel.c2;
function placeSelection(glide = true) {
  const b = box(),
    x = colXs[b.c1],
    w = colXs[b.c2 + 1] - colXs[b.c1],
    y = yOf(b.r1),
    h = (b.r2 - b.r1 + 1) * RH;
  [frameEl, rangeEl, handle].forEach((e) => e.classList.toggle('still', !glide || reduce));
  frameEl.style.transform = `translate(${x}px, ${y}px)`;
  frameEl.style.width = w + 'px';
  frameEl.style.height = h + 'px';
  rangeEl.style.transform = `translate(${x}px, ${y}px)`;
  rangeEl.style.width = w + 'px';
  rangeEl.style.height = h + 'px';
  rangeEl.hidden = isOne();
  handle.style.transform = `translate(${x + w}px, ${y + h}px)`;
  markFrozen();
  syncDock();
}
function markFrozen() {
  const b = box();
  [...frozenEl.children].forEach((e) => e.classList.remove('picked'));
  if (sheet().headerOn && b.r1 === 0)
    [...frozenEl.querySelectorAll('.sh-c')].forEach((e) => {
      const left = parseFloat(e.style.left);
      if (left >= colXs[b.c1] && left < colXs[b.c2 + 1]) e.classList.add('picked');
    });
}
function select(r, c, r2 = r, c2 = c, glide = true, follow = true) {
  const maxR = displayCount() - 1,
    maxC = shownCols(sheet()) - 1;
  sel = { r: clamp(r, 0, maxR), c: clamp(c, 0, maxC), r2: clamp(r2, 0, maxR), c2: clamp(c2, 0, maxC) };
  sheet().sel = sel;
  placeSelection(glide);
  if (follow) reveal(sel.r2, sel.c2);
}
function reveal(d, c) {
  const pad = 8,
    top = yOf(d),
    vh =
      stage.clientHeight
      - parseFloat(getComputedStyle(root).getPropertyValue('--head-h') || 53)
      - LH
      - (hdr() ? RH : 0)
      - 96;
  if (d >= hdr()) {
    if (top < stage.scrollTop) stage.scrollTop = top;
    else if (top + RH > stage.scrollTop + vh) stage.scrollTop = top + RH - vh;
  } else stage.scrollTop = 0;
  const left = colXs[c],
    right = colXs[c + 1],
    rw = stage.clientWidth - padNow();
  if (left - NO < stage.scrollLeft) stage.scrollLeft = left - NO - pad;
  else if (right > stage.scrollLeft + rw) stage.scrollLeft = right - rw + pad;
}
const panelOn = () => filterSide.classList.contains('on') || infoSide.classList.contains('on');
const padNow = () => (panelOn() ? 288 : 0);
let dockMode = '';
function setDockMode(mode, change) {
  if (mode === dockMode) {
    change();
    return;
  }
  const first = !dockMode;
  dockMode = mode;
  if (first || reduce) {
    change();
    return;
  }
  pill.getAnimations().forEach((a) => a.cancel());
  const w0 = pill.getBoundingClientRect().width;
  change();
  const w1 = pill.getBoundingClientRect().width;
  if (Math.abs(w0 - w1) < 1) return;
  pill.style.overflow = 'hidden';
  const a = pill.animate([{ width: w0 + 'px' }, { width: w1 + 'px' }], MOVE);
  a.onfinish = a.oncancel = () => {
    pill.style.overflow = '';
  };
  const fresh = mode === 'formula' ? barWrap : mode === 'many' ? statsEl : null;
  if (fresh) fresh.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 60, fill: 'backwards' });
}
const barMeter = document.createElement('canvas').getContext('2d');
function fitBar() {
  const cs = getComputedStyle(barInput);
  barMeter.font = cs.fontWeight + ' ' + cs.fontSize + ' ' + cs.fontFamily;
  barWrap.style.width =
    clamp(Math.ceil(barMeter.measureText(barInput.value).width) + 16, 96, Math.min(420, innerWidth * 0.5)) + 'px';
}
function tweenDock(change) {
  if (reduce || drag) {
    change();
    return;
  }
  pill.getAnimations().forEach((a) => a.cancel());
  const w0 = pill.getBoundingClientRect().width;
  change();
  const w1 = pill.getBoundingClientRect().width;
  if (Math.abs(w0 - w1) < 1) return;
  pill.style.overflow = 'hidden';
  const a = pill.animate([{ width: w0 + 'px' }, { width: w1 + 'px' }], MOVE);
  a.onfinish = a.oncancel = () => {
    pill.style.overflow = '';
  };
}
function syncDock() {
  if (!opened) return;
  const s = sheet(),
    b = box(),
    one = isOne(),
    here_ = cellRaw(s, fileRow(sel.r), sel.c),
    isF = !!(here_ && typeof here_ === 'object' && here_.f != null);
  setDockMode(!one ? 'many' : isF ? 'formula' : 'plain', () => {
    root.classList.toggle('many', !one);
    root.classList.toggle('plain-cell', one && !isF);
  });
  refEl.textContent = one
    ? colName(sel.c) + (fileRow(sel.r) + 1)
    : `${colName(b.c1)}${fileRow(b.r1) + 1}:${colName(b.c2)}${fileRow(b.r2) + 1}`;
  if (one) statsEl.textContent = '';
  const put = (k, v) => {
    const e = el('span', 'sh-stat');
    e.append(el('i', '', T(k)), el('b', '', v));
    return e;
  };
  if (one) {
    if (document.activeElement !== barInput) {
      barInput.value = sourceOf(here_);
      fitBar();
    }
  } else {
    let sum = 0,
      nums = 0,
      filled = 0;
    for (let d = b.r1; d <= b.r2; d++) {
      const r = fileRow(d);
      if (r >= s.rows.length) continue;
      for (let c = b.c1; c <= b.c2; c++) {
        let v;
        try {
          v = valueOf(s, r, c);
        } catch {
          continue;
        }
        if (v === '') continue;
        filled++;
        if (typeof v === 'number') {
          sum += v;
          nums++;
        }
      }
    }
    const paintStats = () => {
      statsEl.textContent = '';
      if (nums) statsEl.append(put('求和', plain(sum)), put('平均值', plain(sum / nums)));
      statsEl.append(put('计数', String(filled)));
    };
    if (dockMode === 'many' && !drag) tweenDock(paintStats);
    else paintStats();
  }
  const active = hasView(s);
  root.classList.toggle('viewing', active);
  if (active)
    viewChip.textContent = T('显示 {shown} / {all} 行 · 清除', {
      shown: dataRows(s).length,
      all: s.rows.length - hdr(),
    });
  undoBtn.disabled = hAt <= 0;
  redoBtn.disabled = hAt >= history.length - 1;
  barInput.readOnly = readOnly;
  const rows = s.rows.length,
    cols = colCount(s);
  countEl.textContent = T('{rows} 行 · {cols} 列', { rows: num(rows), cols });
}

// ---------- pointer ----------
function hit(e) {
  const b = bodyEl.getBoundingClientRect(),
    x = e.clientX - b.left,
    y = e.clientY - b.top;
  let c = 0;
  while (c < colXs.length - 2 && colXs[c + 1] <= x) c++;
  return { c, d: clamp(Math.floor(y / RH) + hdr(), 0, displayCount() - 1) };
}
let drag = null;
bodyEl.addEventListener('pointerdown', (e) => {
  if (e.button) return;
  if (editing) finishEdit(true);
  shell.closePop();
  if (e.target === handle) {
    e.preventDefault();
    if (readOnly) {
      refuse();
      return;
    }
    bodyEl.setPointerCapture(e.pointerId);
    drag = { t: 'fill', start: { ...box() } };
    return;
  }
  if (e.target.classList.contains('sh-no')) {
    const d = +e.target.parentElement.dataset.d;
    bodyEl.setPointerCapture(e.pointerId);
    select(d, 0, d, shownCols(sheet()) - 1, true, false);
    drag = { t: 'rows', d };
    e.preventDefault();
    return;
  }
  const h = hit(e);
  if (e.shiftKey) select(sel.r, sel.c, h.d, h.c);
  else select(h.d, h.c);
  bodyEl.setPointerCapture(e.pointerId);
  drag = { t: 'cells' };
  e.preventDefault();
  stage.focus({ preventScroll: true });
});
bodyEl.addEventListener('pointermove', (e) => {
  if (!drag) return;
  const h = hit(e);
  if (drag.t === 'cells') {
    if (h.d !== sel.r2 || h.c !== sel.c2) select(sel.r, sel.c, h.d, h.c, false);
  } else if (drag.t === 'rows') select(drag.d, 0, h.d, shownCols(sheet()) - 1, false, false);
  else if (drag.t === 'fill') {
    const b = drag.start,
      down = h.d > b.r2 ? h.d - b.r2 : 0,
      right = h.c > b.c2 ? h.c - b.c2 : 0;
    drag.to = down >= right ? { r2: Math.max(b.r2, h.d), c2: b.c2 } : { r2: b.r2, c2: Math.max(b.c2, h.c) };
    const x = colXs[b.c1],
      y = yOf(b.r1);
    frameEl.classList.add('still');
    frameEl.style.transform = `translate(${x}px, ${y}px)`;
    frameEl.style.width = colXs[drag.to.c2 + 1] - x + 'px';
    frameEl.style.height = (drag.to.r2 - b.r1 + 1) * RH + 'px';
  }
});
const endDrag = () => {
  if (drag && drag.t === 'fill') {
    if (drag.to) fill(drag.start, drag.to);
    else placeSelection();
  }
  drag = null;
};
bodyEl.addEventListener('pointerup', endDrag);
bodyEl.addEventListener('pointercancel', endDrag);
bodyEl.addEventListener('dblclick', (e) => {
  if (e.target === handle || e.target.classList.contains('sh-no')) return;
  startEdit();
});
frozenEl.addEventListener('pointerdown', (e) => {
  if (e.button || !sheet().headerOn) return;
  if (editing) finishEdit(true);
  const b = bodyEl.getBoundingClientRect();
  let c = 0;
  const x = e.clientX - b.left;
  while (c < colXs.length - 2 && colXs[c + 1] <= x) c++;
  if (e.target.classList.contains('sh-no')) select(0, 0, 0, shownCols(sheet()) - 1, true, false);
  else select(0, c, 0, c, true, false);
  e.preventDefault();
});
frozenEl.addEventListener('dblclick', () => startEdit());
// the letters: click takes the column, drag the edge sets its width, the small arrow opens what you can do with it
let gripDrag = null;
lettersEl.addEventListener('pointerdown', (e) => {
  if (e.button) return;
  if (editing) finishEdit(true);
  const g = e.target.dataset && e.target.dataset.grip;
  if (g != null) {
    e.preventDefault();
    lettersEl.setPointerCapture(e.pointerId);
    gripDrag = { c: +g, x: e.clientX, w: widthOf(+g) };
    root.classList.add('resizing');
    return;
  }
  if (e.target.closest('.sh-more')) return;
  const l = e.target.closest('.sh-letter');
  if (!l) {
    if (e.target.classList.contains('sh-corner')) select(0, 0, displayCount() - 1, shownCols(sheet()) - 1, true, false);
    return;
  }
  const c = +l.dataset.c;
  select(0, c, displayCount() - 1, c, true, false);
});
lettersEl.addEventListener('pointermove', (e) => {
  if (!gripDrag) return;
  sheet().widths[gripDrag.c] = clamp(gripDrag.w + e.clientX - gripDrag.x, MINW, 560);
  measureCols();
  gridEl.style.width = colXs[colXs.length - 1] + 'px';
  drawLetters();
  paint(true);
  placeSelection(false);
});
const endGrip = () => {
  if (!gripDrag) return;
  gripDrag = null;
  root.classList.remove('resizing');
};
lettersEl.addEventListener('pointerup', endGrip);
lettersEl.addEventListener('pointercancel', endGrip);
lettersEl.addEventListener('dblclick', (e) => {
  const g = e.target.dataset && e.target.dataset.grip;
  if (g != null) fitColumn(+g);
});
lettersEl.addEventListener('click', (e) => {
  const m = e.target.closest('.sh-more');
  if (m) {
    sound();
    columnMenu(+m.dataset.more, m);
  }
});
lettersEl.addEventListener('contextmenu', (e) => {
  const l = e.target.closest('.sh-letter');
  if (l) {
    e.preventDefault();
    columnMenu(+l.dataset.c, l);
  }
});
bodyEl.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const h = hit(e);
  const b = box();
  if (h.d < b.r1 || h.d > b.r2 || h.c < b.c1 || h.c > b.c2) select(h.d, h.c);
  rowMenu(sel.r, e);
});
const meter = document.createElement('canvas').getContext('2d');
function autoWidths(s) {
  meter.font = `13px ${getComputedStyle(root).getPropertyValue('--font-text')}`;
  const n = colCount(s),
    limit = Math.min(s.rows.length, 200);
  for (let c = 0; c < n; c++) {
    if (s.widths[c] != null) continue;
    let w = 0;
    for (let r = 0; r < limit; r++) {
      const x = cellRaw(s, r, c);
      if (x == null || x === '') continue;
      w = Math.max(w, meter.measureText(shownText(textOf(s, r, c))).width);
    }
    if (w) s.widths[c] = clamp(Math.ceil(w) + 24, 64, 320);
  }
}
function fitColumn(c) {
  const s = sheet();
  meter.font = `13px ${getComputedStyle(root).getPropertyValue('--font-text')}`;
  let w = 0;
  const limit = Math.min(s.rows.length, 2000);
  for (let r = 0; r < limit; r++) w = Math.max(w, meter.measureText(shownText(textOf(s, r, c))).width);
  s.widths[c] = clamp(Math.ceil(w) + 22, MINW, 560);
  layout();
}
function columnMenu(c, anchor) {
  shell.closePop();
  shell.openPop(
    anchor,
    [
      { value: 'up', label: '升序排列' },
      { value: 'down', label: '降序排列' },
      { value: 'filter', label: '筛选' },
      { sep: true },
      { value: 'left', label: '在左侧插入列' },
      { value: 'right', label: '在右侧插入列' },
      { value: 'del', label: '删除当前列' },
      { sep: true },
      { value: 'fit', label: '自适应列宽' },
    ],
    (v) => {
      const s = sheet();
      if (v === 'up' || v === 'down') {
        s.sort = { c, up: v === 'up' };
        viewChanged();
      } else if (v === 'filter') {
        filterCol = c;
        openFilter();
      } else if (v === 'left') insertCol(c);
      else if (v === 'right') insertCol(c + 1);
      else if (v === 'del') deleteCol(c);
      else fitColumn(c);
    }
  );
}
function rowMenu(d, e) {
  // at the pointer: a menu of its own each time, never the one already open
  shell.openPop(
    { contains: () => false },
    [
      { value: 'up', label: '在上方插入行' },
      { value: 'down', label: '在下方插入行' },
      { value: 'del', label: '删除当前行' },
    ],
    (v) => {
      const r = fileRow(d);
      if (v === 'up') insertRow(r);
      else if (v === 'down') insertRow(r + 1);
      else deleteRow(r);
    },
    { at: [e.clientX, e.clientY] }
  );
}

// ---------- editing ----------
let editing = null;
const editBox = el('div', 'sh-edit'),
  editIn = document.createElement('input');
editIn.type = 'text';
editIn.id = 'cellEdit';
editIn.spellcheck = false;
editIn.autocomplete = 'off';
editBox.appendChild(editIn);
inkfall(editIn);
function startEdit(first) {
  if (readOnly) {
    refuse();
    return;
  }
  const s = sheet(),
    r = fileRow(sel.r),
    c = sel.c;
  select(sel.r, sel.c);
  editing = { r, c, d: sel.r };
  const frozen = s.headerOn && sel.r === 0;
  Object.assign(editBox.style, {
    left: colXs[c] + 'px',
    top: yOf(sel.r) + 'px',
    width: Math.max(widthOf(c), 80) + 'px',
    height: RH + 'px',
  });
  editBox.classList.toggle('in-head', frozen);
  (frozen ? frozenEl : bodyEl).appendChild(editBox);
  if (frozen) editBox.style.top = '0';
  editIn.value = first != null ? first : sourceOf(cellRaw(s, r, c));
  barInput.value = editIn.value;
  setTimeout(() => {
    editIn.focus();
    const n = editIn.value.length;
    editIn.setSelectionRange(n, n);
  }, 0);
}
function finishEdit(keep, move) {
  if (!editing) return;
  const { r, c } = editing;
  editing = null;
  editBox.remove();
  const s = sheet(),
    old = cellRaw(s, r, c),
    t = editIn.value;
  if (keep && t !== sourceOf(old))
    applyCells([{ s: at, r, c, old: old === undefined ? '' : old, now: fromTyped(t, old) }]);
  if (move) select(sel.r + move[0], sel.c + move[1]);
  stage.focus({ preventScroll: true });
}
editIn.addEventListener('input', () => {
  barInput.value = editIn.value;
});
editIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'F4') {
    e.preventDefault();
    cycleRef(editIn);
    return;
  }
  if (e.key === 'Enter') {
    e.preventDefault();
    finishEdit(true, [e.shiftKey ? -1 : 1, 0]);
  } else if (e.key === 'Tab') {
    e.preventDefault();
    finishEdit(true, [0, e.shiftKey ? -1 : 1]);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    finishEdit(false);
  }
});
barInput.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'F4') {
    e.preventDefault();
    cycleRef(barInput);
    return;
  }
  if (e.key === 'Enter' || e.key === 'Tab') {
    e.preventDefault();
    const r = fileRow(sel.r),
      c = sel.c,
      old = cellRaw(sheet(), r, c),
      t = barInput.value;
    if (!readOnly && t !== sourceOf(old))
      applyCells([{ s: at, r, c, old: old === undefined ? '' : old, now: fromTyped(t, old) }]);
    barInput.blur();
    select(sel.r + (e.key === 'Enter' ? 1 : 0), sel.c + (e.key === 'Tab' ? 1 : 0));
    stage.focus({ preventScroll: true });
  } else if (e.key === 'Escape') {
    barInput.blur();
    syncDock();
    stage.focus({ preventScroll: true });
  }
});

// ---------- undo: every change the file would see ----------
// A workbook's sheet also remembers each cell's own XML (`x.src`), kept while the cell is as it was, and its style
// (`x.sty`), kept always; a text table remembers each row as it was written (`raw`). Edits and undo look after both.
let history = [{}],
  hAt = 0,
  savedAt = 0,
  moveSeq = 0;
function afterChange(full = true) {
  cache = new Map();
  book.forEach((s) => {
    s.view = null;
  });
  measureCols();
  bodyEl.style.height = (displayCount() - hdr()) * RH + 'px';
  gridEl.style.width = colXs[colXs.length - 1] + 'px';
  if (full) {
    drawLetters();
    paint(true);
  }
  placeSelection(false);
  shell.changed();
  renderPanels();
}
function pushHist(entry) {
  history = history.slice(0, hAt + 1);
  history.push(entry);
  hAt = history.length - 1;
  if (savedAt > hAt) savedAt = -1;
  afterChange();
}
function setCell(s, r, c, value, src) {
  while (s.rows.length <= r) s.rows.push([]);
  const row = s.rows[r];
  while (row.length <= c) row.push('');
  row[c] = value;
  if (s.x) {
    const so = s.x.src[r] || (s.x.src[r] = []);
    so[c] = src;
    s.x.touched = true;
  }
}
const srcOf = (s, r, c) => (s.x && s.x.src[r] ? s.x.src[r][c] : undefined);
function applyCells(list) {
  if (readOnly) {
    refuse();
    return;
  }
  // the shape too: undo gives a row back its length and the sheet its row count, so a file comes back as it was
  for (const x of list) {
    const s = book[x.s];
    x.oldSrc = srcOf(s, x.r, x.c);
    x.wasTouched = s.x ? s.x.touched : false;
    x.rowsLen = s.rows.length;
    x.rowLen = s.rows[x.r] ? s.rows[x.r].length : -1;
    setCell(s, x.r, x.c, x.now);
  }
  pushHist({ cells: list });
}
const snap = (s) => ({ rows: s.rows, widths: { ...s.widths }, raw: s.raw, x: s.x && { ...s.x } });
const restore = (s, o) => {
  s.rows = o.rows;
  s.widths = { ...o.widths };
  s.raw = o.raw;
  if (o.x) s.x = { ...o.x };
};
function undo() {
  if (editing) finishEdit(true);
  if (hAt <= 0) return;
  const h = history[hAt];
  hAt--;
  if (h.cells)
    for (const x of [...h.cells].reverse()) {
      const s = book[x.s];
      setCell(s, x.r, x.c, x.old, x.oldSrc);
      if (x.rowLen >= 0) s.rows[x.r].length = x.rowLen;
      s.rows.length = x.rowsLen;
      if (s.x) s.x.touched = x.wasTouched;
    }
  else h.swapAll.forEach((o, i) => restore(book[i], o));
  afterChange();
}
function redo() {
  if (hAt >= history.length - 1) return;
  hAt++;
  const h = history[hAt];
  if (h.cells) for (const x of h.cells) setCell(book[x.s], x.r, x.c, x.now);
  else h.afterAll.forEach((o, i) => restore(book[i], o));
  afterChange();
}
// every sheet may point here, so undo has to bring all of them back
function structuralAll(run) {
  if (readOnly) {
    refuse();
    return;
  }
  const before = book.map(snap);
  book.forEach((s) => {
    s.rows = s.rows.map((r) => (r ? r.slice() : []));
    if (s.raw) s.raw = s.raw.map((o) => o && { ...o, orig: o.orig.slice(), q: o.q && o.q.slice() });
    if (s.x)
      s.x = {
        ...s.x,
        src: s.x.src.map((r) => (r ? r.slice() : [])),
        sty: s.x.sty.map((r) => (r ? r.slice() : [])),
        rowAttr: s.x.rowAttr.slice(),
        moves: s.x.moves.slice(),
      };
  });
  run(sheet());
  pushHist({ swapAll: before, afterAll: book.map(snap) });
}
function moveAllFormulas(name, isRow, at_, by) {
  for (const s of book)
    for (let r = 0; r < s.rows.length; r++) {
      const row = s.rows[r];
      if (!row) continue;
      for (let i = 0; i < row.length; i++) {
        const x = row[i];
        if (x && typeof x === 'object' && x.f != null) {
          const f = moveRefs(x.f, name, s.name, isRow, at_, by);
          if (f !== x.f) {
            row[i] = { ...x, f };
            if (s.x) {
              if (s.x.src[r]) s.x.src[r][i] = undefined;
              s.x.touched = true;
            }
          }
        }
      }
    }
}
const noteMove = (s, isRow, at_, by) => {
  if (s.x) {
    s.x.moves.push({ isRow, at: at_, by, seq: ++moveSeq });
    s.x.touched = true;
  }
};
function insertRow(r) {
  structuralAll((s) => {
    s.rows.splice(r, 0, []);
    if (s.raw) s.raw.splice(r, 0, undefined);
    if (s.x) {
      s.x.src.splice(r, 0, []);
      s.x.sty.splice(r, 0, []);
      s.x.rowAttr.splice(r, 0, undefined);
    }
    noteMove(s, true, r, 1);
    moveAllFormulas(s.name, true, r, 1);
  });
}
function deleteRow(r) {
  structuralAll((s) => {
    if (r >= s.rows.length) return;
    s.rows.splice(r, 1);
    if (s.raw) s.raw.splice(r, 1);
    if (s.x) {
      s.x.src.splice(r, 1);
      s.x.sty.splice(r, 1);
      s.x.rowAttr.splice(r, 1);
    }
    noteMove(s, true, r, -1);
    moveAllFormulas(s.name, true, r, -1);
  });
}
function shiftWidths(s, c, by) {
  const w = {};
  for (const k in s.widths) {
    const i = +k;
    if (by < 0 && i === c) continue;
    w[i >= c + (by < 0 ? 1 : 0) ? i + by : i] = s.widths[k];
  }
  s.widths = w;
}
const spliceRow = (arr, c, by) => {
  if (arr && arr.length > c) {
    if (by > 0) arr.splice(c, 0, undefined);
    else arr.splice(c, 1);
  }
};
function insertCol(c) {
  structuralAll((s) => {
    s.rows.forEach((r, i) => {
      if (r.length > c) r.splice(c, 0, '');
      if (s.raw && s.raw[i] && s.raw[i].orig.length > c) {
        s.raw[i].orig.splice(c, 0, '');
        spliceRow(s.raw[i].q, c, 1);
      }
    });
    if (s.x) {
      s.x.src.forEach((r) => spliceRow(r, c, 1));
      s.x.sty.forEach((r) => spliceRow(r, c, 1));
    }
    shiftWidths(s, c, 1);
    noteMove(s, false, c, 1);
    moveAllFormulas(s.name, false, c, 1);
  });
}
function deleteCol(c) {
  structuralAll((s) => {
    s.rows.forEach((r, i) => {
      if (r.length > c) r.splice(c, 1);
      if (s.raw && s.raw[i] && s.raw[i].orig.length > c) {
        s.raw[i].orig.splice(c, 1);
        spliceRow(s.raw[i].q, c, -1);
      }
    });
    if (s.x) {
      s.x.src.forEach((r) => spliceRow(r, c, -1));
      s.x.sty.forEach((r) => spliceRow(r, c, -1));
    }
    shiftWidths(s, c, -1);
    noteMove(s, false, c, -1);
    moveAllFormulas(s.name, false, c, -1);
  });
}
function clearCells() {
  if (readOnly) {
    refuse();
    return;
  }
  const s = sheet(),
    b = box(),
    list = [];
  for (let d = b.r1; d <= b.r2; d++) {
    const r = fileRow(d);
    if (r >= s.rows.length) continue;
    for (let c = b.c1; c <= b.c2; c++) {
      const old = cellRaw(s, r, c);
      if (old != null && old !== '') list.push({ s: at, r, c, old, now: '' });
    }
  }
  if (list.length) applyCells(list);
}
// dragging the corner: a run of numbers carries on, anything else repeats
function fill(b, to) {
  if (readOnly) {
    refuse();
    placeSelection();
    return;
  }
  const s = sheet(),
    list = [],
    down = to.r2 > b.r2;
  const put = (d, c, v) => {
    const r = fileRow(d),
      old = cellRaw(s, r, c);
    list.push({ s: at, r, c, old: old === undefined ? '' : old, now: v });
  };
  const next = (x, dr, dc, offset, seedList) => {
    if (x && typeof x === 'object' && x.f != null) return { f: shiftFormula(x.f, dr, dc), fmt: x.fmt };
    if (seedList.length >= 2 && seedList.every((v) => typeof v === 'string' && looksNumber(v))) {
      const st = +seedList[1] - +seedList[0];
      return String(+(+seedList[seedList.length - 1] + st * offset).toFixed(10));
    }
    if (seedList.length >= 2 && seedList.every((v) => v && typeof v === 'object' && v.f == null)) {
      const st = seedList[1].n - seedList[0].n;
      return { n: +(seedList[seedList.length - 1].n + st * offset).toFixed(10), fmt: seedList[0].fmt };
    }
    return x == null ? '' : x;
  };
  if (down)
    for (let c = b.c1; c <= b.c2; c++) {
      const seed = [];
      for (let d = b.r1; d <= b.r2; d++) seed.push(cellRaw(s, fileRow(d), c));
      for (let d = b.r2 + 1; d <= to.r2; d++) {
        const off = d - b.r2,
          k = (off - 1) % seed.length,
          base = seed[k];
        put(d, c, next(base, fileRow(d) - fileRow(b.r1 + k), 0, off, seed));
      }
    }
  else
    for (let d = b.r1; d <= b.r2; d++) {
      const r = fileRow(d),
        seed = [];
      for (let c = b.c1; c <= b.c2; c++) seed.push(cellRaw(s, r, c));
      for (let c = b.c2 + 1; c <= to.c2; c++) {
        const off = c - b.c2,
          k = (off - 1) % seed.length,
          base = seed[k];
        put(d, c, next(base, 0, c - (b.c1 + k), off, seed));
      }
    }
  sel = { r: b.r1, c: b.c1, r2: to.r2, c2: to.c2 };
  if (list.length) applyCells(list);
  else placeSelection();
}
// Ctrl + arrow: to the end of this run of filled cells, or across the gap to the next one
const isFilled = (d, c) => {
  const s = sheet(),
    r = fileRow(d);
  return r < s.rows.length && textOf(s, r, c) !== '';
};
function edgeFrom(d, c, dr, dc) {
  const lastD = displayCount() - 1,
    lastC = shownCols(sheet()) - 1,
    inside = (a, b) => a >= 0 && a <= lastD && b >= 0 && b <= lastC;
  let a = d + dr,
    b = c + dc;
  if (!inside(a, b)) return [d, c];
  if (isFilled(d, c) && isFilled(a, b)) {
    while (inside(a + dr, b + dc) && isFilled(a + dr, b + dc)) {
      a += dr;
      b += dc;
    }
    return [a, b];
  }
  while (inside(a, b) && !isFilled(a, b)) {
    a += dr;
    b += dc;
  }
  if (inside(a, b)) return [a, b];
  if (dr > 0) return [Math.max(d, hdr() + dataRows(sheet()).length - 1), c];
  if (dc > 0) return [d, Math.max(c, colCount(sheet()) - 1)];
  return [dr ? 0 : d, dc ? 0 : c];
}
// Ctrl+D and Ctrl+R: copy down or right into the selection; a single cell copies from its neighbour
function fillShort(down) {
  const b = box(),
    one = isOne();
  if (down) {
    if (one) {
      if (b.r1 < 1) return;
      fill({ r1: b.r1 - 1, r2: b.r1 - 1, c1: b.c1, c2: b.c2 }, { r2: b.r1, c2: b.c2 });
    } else if (b.r2 > b.r1) fill({ r1: b.r1, r2: b.r1, c1: b.c1, c2: b.c2 }, { r2: b.r2, c2: b.c2 });
  } else if (one) {
    if (b.c1 < 1) return;
    fill({ r1: b.r1, r2: b.r2, c1: b.c1 - 1, c2: b.c1 - 1 }, { r2: b.r2, c2: b.c1 });
  } else if (b.c2 > b.c1) fill({ r1: b.r1, r2: b.r2, c1: b.c1, c2: b.c1 }, { r2: b.r2, c2: b.c2 });
}
// Alt+=: a sum of the numbers right above, or else to the left, ready to confirm
function autoSum() {
  const s = sheet(),
    num = (d, c) => {
      const r = fileRow(d);
      if (r >= s.rows.length) return false;
      try {
        return typeof valueOf(s, r, c) === 'number';
      } catch {
        return false;
      }
    };
  const d = sel.r,
    c = sel.c;
  let a = d - 1,
    text = null;
  while (a >= 0 && num(a, c)) a--;
  if (a + 1 < d) text = `=SUM(${colName(c)}${fileRow(a + 1) + 1}:${colName(c)}${fileRow(d - 1) + 1})`;
  else {
    let l = c - 1;
    while (l >= 0 && num(d, l)) l--;
    if (l + 1 < c) text = `=SUM(${colName(l + 1)}${fileRow(d) + 1}:${colName(c - 1)}${fileRow(d) + 1})`;
  }
  startEdit(text || '=SUM()');
}
// F4 while typing a formula: cycles the reference by the caret through $A$1, A$1, $A1, A1
function cycleRef(input) {
  const v = input.value,
    at_ = input.selectionStart,
    re = /\$?[A-Z]{1,3}\$?\d+/g;
  let m_,
    hitRef = null;
  while ((m_ = re.exec(v))) {
    if (m_.index > at_) break;
    hitRef = m_;
  }
  if (!v.startsWith('=') || !hitRef) return;
  const p = /(\$?)([A-Z]{1,3})(\$?)(\d+)/.exec(hitRef[0]),
    next =
      p[1] && p[3]
        ? `${p[2]}$${p[4]}`
        : !p[1] && p[3]
          ? `$${p[2]}${p[4]}`
          : p[1] && !p[3]
            ? `${p[2]}${p[4]}`
            : `$${p[2]}$${p[4]}`;
  input.value = v.slice(0, hitRef.index) + next + v.slice(hitRef.index + hitRef[0].length);
  const c = hitRef.index + next.length;
  input.setSelectionRange(c, c);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}
// the clipboard speaks tabs, so it goes straight to and from any other spreadsheet
function selectionText() {
  const s = sheet(),
    b = box(),
    lines = [];
  for (let d = b.r1; d <= b.r2; d++) {
    const r = fileRow(d),
      row = [];
    for (let c = b.c1; c <= b.c2; c++) row.push(fileText(s, r, c));
    lines.push(row.join('\t'));
  }
  return lines.join('\r\n');
}
const typingNow = () => editing || /INPUT|TEXTAREA/.test(document.activeElement.tagName);
addEventListener('copy', (e) => {
  if (!opened || typingNow()) return;
  e.preventDefault();
  e.clipboardData.setData('text/plain', selectionText());
});
addEventListener('cut', (e) => {
  if (!opened || typingNow()) return;
  e.preventDefault();
  if (readOnly) {
    refuse();
    return;
  }
  e.clipboardData.setData('text/plain', selectionText());
  clearCells();
});
function pasteText(text) {
  if (readOnly) {
    refuse();
    return;
  }
  const grid = text
      .replace(/\r\n?/g, '\n')
      .replace(/\n$/, '')
      .split('\n')
      .map((l) => l.split('\t')),
    s = sheet(),
    b = box(),
    list = [];
  grid.forEach((row, i) => {
    const r = fileRow(b.r1 + i);
    row.forEach((t, j) => {
      const c = b.c1 + j,
        old = cellRaw(s, r, c);
      list.push({ s: at, r, c, old: old === undefined ? '' : old, now: fromTyped(t, old) });
    });
  });
  sel = { r: b.r1, c: b.c1, r2: b.r1 + grid.length - 1, c2: b.c1 + grid[0].length - 1 };
  applyCells(list);
}
addEventListener('paste', (e) => {
  if (!opened || typingNow()) return;
  const t = e.clipboardData.getData('text/plain');
  if (t) {
    e.preventDefault();
    pasteText(t);
  }
});

// ---------- filter and sort: the panel. The file underneath stays as it is ----------
let filterCol = 0,
  filterSearch = '';
function viewChanged() {
  const s = sheet();
  s.view = null;
  stage.scrollTop = 0;
  select(hdr(), sel.c, hdr(), sel.c, false);
  layout();
  renderPanels();
}
function clearView() {
  const s = sheet();
  s.sort = null;
  s.filters = {};
  viewChanged();
}
function openFilter() {
  closeFind();
  if (!filterSide.classList.contains('on')) filterBtn.click();
  else renderPanels();
}
const heading = (s, c) => (s.headerOn ? shownText(textOf(s, 0, c)) : '') || colName(c);
function panelFilter(body) {
  const s = sheet(),
    c = filterCol,
    cols = el('div', 'sh-colpick'),
    now = el('button', 'btn soft sh-colbtn dyn');
  now.type = 'button';
  now.id = 'filterCol';
  now.innerHTML = '<span></span>' + icon('chevron');
  now.firstChild.textContent = heading(s, c);
  now.addEventListener('mousedown', (e) => e.preventDefault());
  now.addEventListener('click', () => {
    sound();
    const n = colCount(s);
    shell.openPop(
      now,
      Array.from({ length: n }, (_, i) => ({ value: i, label: heading(s, i), on: i === c, raw: true })),
      (v) => {
        filterCol = v;
        filterSearch = '';
        renderPanels();
      }
    );
  });
  cols.appendChild(now);
  body.appendChild(cols);
  // sort: the family's sliding capsule
  const seg = el('div', 'seg'),
    pl = el('i', 'pl');
  seg.appendChild(pl);
  const cur = s.sort && s.sort.c === c ? (s.sort.up ? 'up' : 'down') : 'none';
  const btns = [
    ['none', '默认顺序'],
    ['up', '升序'],
    ['down', '降序'],
  ].map(([k, t]) => {
    const b = el('button', 'sg dyn', T(t));
    b.type = 'button';
    b.id = 'sort-' + k;
    b.setAttribute('aria-pressed', String(k === cur));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      s.sort = k === 'none' ? null : { c, up: k === 'up' };
      btns.forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      place();
      s.view = null;
      stage.scrollTop = 0;
      select(hdr(), sel.c, hdr(), sel.c, false);
      layout();
    });
    seg.appendChild(b);
    return b;
  });
  // a row whose words don't fit wraps (Predeterminado / Ascendente / Descendente), as Slides' longer groups do; the pill follows
  const place = () => {
    const on = btns.find((b) => b.getAttribute('aria-pressed') === 'true');
    seg.classList.toggle(
      'rows',
      btns.some((b) => b.offsetTop !== btns[0].offsetTop)
    );
    pl.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop - 3}px)`;
    pl.style.width = on.offsetWidth + 'px';
    pl.style.height = on.offsetHeight + 'px';
  };
  body.appendChild(seg);
  pl.style.transition = 'none';
  requestAnimationFrame(() => {
    place();
    void pl.offsetWidth;
    pl.style.transition = '';
  });
  // values: what this column actually holds
  const counts = new Map(),
    first = s.headerOn ? 1 : 0;
  for (let r = first; r < s.rows.length; r++) {
    const t = shownText(textOf(s, r, c));
    counts.set(t, (counts.get(t) || 0) + 1);
  }
  const all = [...counts.keys()].sort((a, b) => a.localeCompare(b, 'zh-CN', { numeric: true }));
  const find = el('span', 'fi sh-fsearch'),
    input = document.createElement('input');
  input.type = 'text';
  input.id = 'filterSearch';
  input.placeholder = T('搜索筛选内容');
  input.spellcheck = false;
  input.value = filterSearch;
  find.appendChild(input);
  body.appendChild(find);
  inkfall(input);
  const list = el('div', 'sh-values');
  list.id = 'filterValues';
  const draw = () => {
    list.textContent = '';
    const q = filterSearch.trim().toLowerCase(),
      showing = all.filter((v) => !q || v.toLowerCase().includes(q)).slice(0, 300);
    showing.forEach((v) => {
      const b = el('button', 'opt sh-val');
      b.type = 'button';
      const on = !s.filters[c] || s.filters[c].has(v);
      b.setAttribute('aria-pressed', String(on));
      b.innerHTML =
        '<span></span><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path class="tick" pathLength="1" d="M4 12l5 5L20 6"/></svg>';
      b.firstChild.textContent = v || T('（空白）');
      b.dataset.v = v;
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        const set = s.filters[c] ? new Set(s.filters[c]) : new Set(all);
        if (set.has(v)) set.delete(v);
        else set.add(v);
        if (set.size === all.length) delete s.filters[c];
        else s.filters[c] = set;
        b.setAttribute('aria-pressed', String(set.has(v)));
        s.view = null;
        stage.scrollTop = 0;
        select(hdr(), sel.c, hdr(), sel.c, false);
        layout();
      });
      list.appendChild(b);
    });
  };
  draw();
  input.addEventListener('input', () => {
    filterSearch = input.value;
    draw();
  });
  input.addEventListener('keydown', (e) => e.stopPropagation());
  body.appendChild(list);
  const row = el('div', 'side-row'),
    allBtn = el('button', 'btn soft dyn', T('全选')),
    noneBtn = el('button', 'btn soft dyn', T('清除条件'));
  allBtn.type = noneBtn.type = 'button';
  allBtn.id = 'filterAll';
  noneBtn.id = 'filterNone';
  [allBtn, noneBtn].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
  allBtn.addEventListener('click', () => {
    sound();
    delete s.filters[c];
    s.view = null;
    draw();
    stage.scrollTop = 0;
    layout();
  });
  noneBtn.addEventListener('click', () => {
    sound();
    s.filters[c] = new Set();
    s.view = null;
    draw();
    stage.scrollTop = 0;
    layout();
  });
  row.append(allBtn, noneBtn);
  body.appendChild(row);
}

// ---------- the panels: the family's side panels, one per button ----------
function renderPanels() {
  for (const [side, info] of [
    [filterSide, false],
    [infoSide, true],
  ]) {
    if (!side.classList.contains('on') || !opened) continue;
    side.textContent = '';
    const body = el('div', 'side-body');
    side.append(el('h3', 'dyn', T(info ? '信息' : '筛选与排序')), body);
    if (info) panelInfo(body);
    else panelFilter(body);
  }
  syncPads();
}
function panelInfo(body) {
  const s = sheet(),
    dl = el('dl', 'info-list'),
    row = (k, v) => {
      if (v == null || v === '') return;
      const d = el('div');
      d.append(el('dt', 'dyn', T(k)), el('dd', 'dyn', v));
      dl.appendChild(d);
    };
  // a database's table that was cut: the rows shown of the rows it has (its header row counted in both, as everywhere)
  row('格式', FORMAT_NAMES[kind] || kind.toUpperCase());
  if (book.length > 1) row('工作表数量', String(book.length));
  row('行数', s.cut ? num(s.rows.length) + ' / ' + num(s.cut + 1) : num(s.rows.length));
  row('列数', String(colCount(s)));
  row('文件大小', money(fileSize));
  if (csvInfo) row('分隔符', T(csvInfo.d === ',' ? '逗号 ( , )' : csvInfo.d === '\t' ? '制表符 (Tab)' : '分号 ( ; )'));
  if (extras) {
    if (extras.charts) row('内嵌图表', String(extras.charts));
    if (extras.pivots) row('数据透视表', String(extras.pivots));
    if (extras.macros) row('宏与脚本', T('包含（出于安全考量已禁用执行）'));
  }
  body.appendChild(dl);
  if (csvInfo) {
    const box_ = el('div'),
      chips = el('div', 'chips sh-enc');
    [
      ['utf-8', 'UTF-8'],
      ['gbk', 'GBK'],
      ['big5', 'Big5'],
      ['shift_jis', 'SJIS'],
    ].forEach(([k, t]) => {
      const b = el('button', 'chip', t);
      b.type = 'button';
      b.id = 'enc-' + k;
      b.setAttribute('aria-pressed', String(k === csvInfo.enc));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        rereadAs(k);
      });
      chips.appendChild(b);
    });
    box_.append(el('span', 'side-label dyn', T('字符编码')), chips);
    body.appendChild(box_);
  }
}
// the same bytes read in another encoding: what fixes a garbled CSV. Unsaved edits are asked about first
function rereadAs(enc) {
  if (!csvInfo || enc === csvInfo.enc) return;
  shell.guard(async () => {
    const d = await own.sheetText(enc);
    if (!d) return;
    const s = sheet(),
      headerOn = s.headerOn,
      p = parseDelimited(d.text, csvInfo.d);
    csvInfo = { ...csvInfo, enc: d.enc, bom: d.bom, eol: lineEndOf(d.text), finalEol: /[\r\n]$/.test(d.text) };
    s.rows = p.rows;
    s.raw = p.raw;
    s.headerOn = headerOn;
    s.view = null;
    s.widths = {};
    autoWidths(s);
    cache = new Map();
    history = [{}];
    hAt = 0;
    savedAt = 0;
    layout();
    shell.changed();
    renderPanels();
    if (!reduce) rowsEl.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  });
}
function syncPads() {
  root.style.setProperty('--pad-r', (panelOn() ? 288 : 0) + 'px');
  root.style.setProperty('--gut', stage.offsetWidth - stage.clientWidth + 'px');
}
// a panel's button is the family's; what goes in it is drawn as it opens
filterBtn.addEventListener('click', () => {
  if (filterSide.classList.contains('on')) {
    filterCol = sel.c;
    filterSearch = '';
    closeFind();
  }
  renderPanels();
});
infoBtn.addEventListener('click', () => {
  if (infoSide.classList.contains('on')) closeFind();
  renderPanels();
});
new MutationObserver(syncPads).observe(filterSide, { attributes: true, attributeFilter: ['class'] });
new MutationObserver(syncPads).observe(infoSide, { attributes: true, attributeFilter: ['class'] });

// ---------- sheets: the family's sliding capsule, next to the file's name ----------
let tabBtns = [];
function drawTabs() {
  [...tabSeg.querySelectorAll('.sg')].forEach((b) => b.remove());
  tabSeg.hidden = book.length < 2;
  tabBtns = book.map((s, i) => {
    const b = el('button', 'sg dyn', s.name);
    b.type = 'button';
    b.id = 'tab-' + i;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      switchSheet(i);
    });
    tabSeg.appendChild(b);
    return b;
  });
  placeTabs();
}
function placeTabs() {
  tabBtns.forEach((b, i) => b.setAttribute('aria-pressed', String(i === at)));
  const on = tabBtns[at];
  if (on && !tabSeg.hidden) {
    tabPill.style.transform = `translateX(${on.offsetLeft}px)`;
    tabPill.style.width = on.offsetWidth + 'px';
  }
}
function switchSheet(i) {
  if (i === at) return;
  if (editing) finishEdit(true);
  at = i;
  sel = sheet().sel;
  stage.scrollTop = 0;
  stage.scrollLeft = 0;
  placeTabs();
  closeFindHits();
  layout();
  renderPanels();
  if (!reduce) gridEl.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}

// ---------- find and replace: a glass box over the table ----------
const findBox = el('div', 'cfind');
findBox.setAttribute('role', 'search');
findBox.id = 'findBox';
const fWrap = (id, ph) => {
  const w = el('span', 'fi'),
    i = document.createElement('input');
  i.type = 'text';
  i.id = id;
  i.dataset.ph = ph;
  i.spellcheck = false;
  i.autocomplete = 'off';
  w.appendChild(i);
  inkfall(i);
  return { w, i };
};
const fq = fWrap('findText', '查找'),
  fr = fWrap('replaceText', '替换为'),
  findCount = el('span', 'f-count dyn');
const fBtn = (id, n, tip, run) => {
  const b = el('button', 'f-b');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(n);
  b.setAttribute('aria-label', tip);
  b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    run(b);
  });
  return b;
};
const fMore = fBtn('findMore', 'next', '替换', (b) => {
  const on = findBox.classList.toggle('rep');
  b.setAttribute('aria-pressed', String(on));
  if (on) fr.i.focus();
});
fMore.classList.add('f-more');
fMore.setAttribute('aria-pressed', 'false');
const fRow = el('div', 'f-row'),
  fRep = el('div', 'f-row f-rep'),
  fRepBox = el('div', 'f-repbox');
const oneBtn = el('button', 'btn soft f-t', '替换'),
  allBtn = el('button', 'btn soft f-t', '全部替换');
oneBtn.type = allBtn.type = 'button';
oneBtn.id = 'replaceOne';
allBtn.id = 'replaceAll';
[oneBtn, allBtn].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
fRow.append(
  fMore,
  fq.w,
  findCount,
  fBtn('findPrev', 'up', '上一个', () => stepFind(-1)),
  fBtn('findNext', 'down', '下一个', () => stepFind(1)),
  fBtn('findClose', 'close', '关闭', () => closeFind())
);
fRep.append(fr.w, oneBtn, allBtn);
fRepBox.appendChild(fRep);
findBox.append(fRow, fRepBox);
shell.place(findBox);
shell.translates(findBox);
let hits = [],
  hitAt = -1;
const closeFindHits = () => {
  hits = [];
  hitAt = -1;
  findCount.textContent = '';
};
function runFind() {
  const q = fq.i.value.trim().toLowerCase(),
    s = sheet();
  hits = [];
  hitAt = -1;
  if (q && s) {
    const total = hdr() + dataRows(s).length;
    for (let d = 0; d < total; d++) {
      const r = fileRow(d),
        row = s.rows[r];
      if (!row) continue;
      for (let c = 0; c < row.length; c++)
        if (
          shownText(textOf(s, r, c))
            .toLowerCase()
            .includes(q)
        )
          hits.push([d, c]);
    }
    if (hits.length) {
      const i = hits.findIndex(([d, c]) => d > sel.r || (d === sel.r && c >= sel.c));
      hitAt = i >= 0 ? i : 0;
      select(hits[hitAt][0], hits[hitAt][1]);
    }
  }
  findCount.textContent = !q ? '' : hits.length ? `${hitAt + 1}/${hits.length}` : T('无匹配结果');
  findBox.classList.toggle('none', !!q && !hits.length);
}
function stepFind(k) {
  if (!hits.length) return;
  hitAt = (hitAt + k + hits.length) % hits.length;
  select(hits[hitAt][0], hits[hitAt][1]);
  findCount.textContent = `${hitAt + 1}/${hits.length}`;
}
function openFind() {
  if (!opened) return;
  shell.closeAll();
  findBox.classList.add('on');
  findBtn.setAttribute('aria-pressed', 'true');
  setTimeout(() => {
    fq.i.focus();
    fq.i.select();
  }, 30);
}
function closeFind() {
  if (!findBox.classList.contains('on')) return false;
  findBox.classList.remove('on', 'rep', 'none');
  fMore.setAttribute('aria-pressed', 'false');
  findBtn.setAttribute('aria-pressed', 'false');
  fq.i.value = '';
  closeFindHits();
  fq.i.blur();
  stage.focus({ preventScroll: true });
  return true;
}
const swap = (t, q, to) => t.replace(new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi'), () => to);
oneBtn.addEventListener('click', () => {
  sound();
  const h = hits[hitAt];
  if (!h) return;
  if (readOnly) {
    refuse();
    return;
  }
  const s = sheet(),
    r = fileRow(h[0]),
    old = cellRaw(s, r, h[1]);
  applyCells([{ s: at, r, c: h[1], old, now: fromTyped(swap(sourceOf(old), fq.i.value.trim(), fr.i.value), old) }]);
  runFind();
});
allBtn.addEventListener('click', () => {
  sound();
  const q = fq.i.value.trim();
  if (!hits.length) return;
  if (readOnly) {
    refuse();
    return;
  }
  const s = sheet(),
    list = hits
      .map(([d, c]) => {
        const r = fileRow(d),
          old = cellRaw(s, r, c);
        return { s: at, r, c, old, now: fromTyped(swap(sourceOf(old), q, fr.i.value), old) };
      })
      .filter((x) => sourceOf(x.old) !== sourceOf(x.now));
  if (list.length) applyCells(list);
  runFind();
});
fq.i.addEventListener('input', () => {
  clearTimeout(runFind.t);
  runFind.t = setTimeout(runFind, 120);
});
[fq.i, fr.i].forEach((i) =>
  i.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      stepFind(e.shiftKey ? -1 : 1);
    } else if (e.key === 'Escape') closeFind();
  })
);
findBtn.addEventListener('click', () => {
  sound();
  if (!closeFind()) openFind();
});
// opening the family's card or settings closes find
['settings', 'saveAs'].forEach((id) => document.getElementById(id).addEventListener('click', closeFind));

// ---------- save as: the family's card with this tool's rows ----------
let encKey = 'utf-8',
  keepView = false;
const anyFormula = () => sheet().rows.some((r) => r && r.some((x) => x && typeof x === 'object' && x.f != null));
function saveNote(fmt) {
  if (fmt === 'xlsx') return readOnly ? '可编辑的新文档，原文件不变。' : '';
  if (book.length > 1) return '仅导出当前激活的工作表，单元格公式将转换为静态计算结果。';
  if (anyFormula()) return '单元格公式将转换为静态计算结果。';
  return '';
}
const textOrigin = () => origin === 'csv' || origin === 'tsv';
const encodings = () => {
  const own_ =
    csvInfo && !['utf-8', 'gbk'].includes(csvInfo.enc)
      ? [
          [
            csvInfo.enc,
            csvInfo.enc === 'big5' ? 'Big5' : csvInfo.enc === 'shift_jis' ? 'SJIS' : csvInfo.enc.toUpperCase(),
          ],
        ]
      : [];
  return [['utf-8', 'UTF-8'], ['gbk', 'GBK'], ...own_];
};
function saveRows(rows, fmt, again) {
  if (fmt !== 'xlsx') {
    const box_ = el('div'),
      seg = el('div', 'seg'),
      pl = el('i', 'pl');
    seg.appendChild(pl);
    const btns = encodings().map(([k, t]) => {
      const b = el('button', 'sg', t);
      b.type = 'button';
      b.id = 'out-' + k;
      b.setAttribute('aria-pressed', String(k === encKey));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        encKey = k;
        btns.forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
        place();
        again();
      });
      seg.appendChild(b);
      return b;
    });
    const place = () => {
      const on = btns.find((b) => b.getAttribute('aria-pressed') === 'true') || btns[0];
      pl.style.transform = `translateX(${on.offsetLeft}px)`;
      pl.style.width = on.offsetWidth + 'px';
    };
    box_.append(el('span', 'side-label', T('字符编码')), seg);
    rows.appendChild(box_);
    pl.style.transition = 'none';
    requestAnimationFrame(() => {
      place();
      void pl.offsetWidth;
      pl.style.transition = '';
    });
  }
  if (hasView()) {
    const t = el('button', 'row compact');
    t.type = 'button';
    t.id = 'keepView';
    t.setAttribute('role', 'switch');
    t.setAttribute('aria-checked', String(keepView));
    t.innerHTML = '<span class="row-t"><b></b></span><i class="tog"></i>';
    t.querySelector('b').textContent = T('仅导出当前筛选与排序后的可见数据');
    t.addEventListener('mousedown', (e) => e.preventDefault());
    t.addEventListener('click', () => {
      sound();
      keepView = !keepView;
      t.setAttribute('aria-checked', String(keepView));
      again();
    });
    rows.appendChild(t);
  }
}
// the card opens on the file's own encoding, and the view is saved as it is only when asked
document.getElementById('saveAs').addEventListener('click', resetCard, true);
fileBtn.addEventListener('click', resetCard, true);
function resetCard() {
  encKey = csvInfo ? csvInfo.enc : 'utf-8';
  if (!encodings().some(([k]) => k === encKey)) encKey = 'utf-8';
  keepView = false;
}
// when the card closes, the keys belong to the table again (the card keeps its keys to itself)
const dlgEl = document.querySelector('.dlg');
new MutationObserver(() => {
  if (!dlgEl.classList.contains('on') && dlgEl.contains(document.activeElement) && opened)
    stage.focus({ preventScroll: true });
}).observe(dlgEl, { attributes: true, attributeFilter: ['class'] });
const exporting = () => keepView && hasView();
// a text table's rows for a file: the cells' own words, a result for a formula
function outputRows(fmt) {
  const s = sheet(),
    list = exporting() ? [...(s.headerOn ? [0] : []), ...dataRows(s)] : s.rows.map((_, r) => r),
    n = colCount(s);
  const keepShape = textOrigin() && !exporting();
  return list.map((r) => {
    const row = s.rows[r] || [],
      out = [],
      len = keepShape ? row.length : n;
    for (let c = 0; c < len; c++) {
      const x = row[c];
      out.push(typeof x === 'string' ? x : x == null ? '' : fileText(s, r, c));
    }
    void fmt;
    return out;
  });
}
function textFor(fmt) {
  const d = fmt === 'tsv' ? '\t' : csvInfo && csvInfo.d !== '\t' ? csvInfo.d : ',';
  // rows of the file it came from go back as they were written: same delimiter, same table, nothing filtered
  const sameTable = textOrigin() && !exporting() && csvInfo && d === csvInfo.d && at === 0;
  const eol = csvInfo ? csvInfo.eol : '\r\n';
  return writeDelimited(outputRows(fmt), d, eol, sameTable ? sheet().raw : null, csvInfo ? csvInfo.finalEol : true);
}
const bomFor = () => (csvInfo && encKey === csvInfo.enc ? csvInfo.bom : encKey === 'utf-8' && !csvInfo);
// a formula's result for the file, or its error
const valueForFile = (s, r, c) => valueOf(s, r, c);
function exportBook() {
  const s = sheet(),
    rowsOut = [...(s.headerOn ? [0] : []), ...dataRows(s)].map((r) =>
      (s.rows[r] || []).map((x, c) => (x && typeof x === 'object' && x.f != null ? fileText(s, r, c) : x))
    );
  return [{ name: s.name, rows: rowsOut, widths: s.widths }];
}
async function estimate(fmt) {
  if (fmt === 'xlsx')
    return pkg && !exporting()
      ? fileSize
      : Math.max(
          4096,
          book.reduce((a, s) => a + s.rows.length * colCount(s) * 9, 0)
        );
  const t = textFor(fmt);
  return t.length > 4e6 ? t.length * 1.4 : new TextEncoder().encode(t).length;
}
async function saveTo(path, fmt) {
  const exp = exporting();
  let r;
  if (fmt === 'xlsx') {
    const fresh = exp || !pkg,
      p = exp
        ? freshXlsx(exportBook(), () => '')
        : pkg
          ? patchXlsx(book, pkg, valueForFile)
          : freshXlsx(book, valueForFile, nameList);
    r = await own.saveXlsx(path, p.parts, p.remove, fresh, exp);
  } else r = await own.saveText(path, textFor(fmt), encKey, bomFor(), exp);
  if (r.status !== 'saved' || !exp) {
    if (r.status === 'saved') lastSave = { fmt, enc: encKey, bom: bomFor() };
    return r;
  }
  // the filtered and sorted copy isn't the file: the page stays on what's open
  return { ...r, exported: true };
}
let lastSave = null;
function afterSave(r, fmt) {
  if (r.exported) return;
  savedAt = hAt;
  kind = fmt;
  fileSize = r.size || fileSize;
  if (fmt !== 'xlsx' && lastSave)
    csvInfo = {
      ...(csvInfo || { d: fmt === 'tsv' ? '\t' : ',', eol: '\r\n', finalEol: true }),
      enc: lastSave.enc,
      bom: lastSave.bom,
      d: fmt === 'tsv' ? '\t' : csvInfo && csvInfo.d !== '\t' ? csvInfo.d : ',',
    };
  if (readOnly) {
    readOnly = false;
    showReadOnly(false);
  }
  // what was saved is what was shown: a cut table is whole in its new file
  book.forEach((s) => {
    delete s.cut;
  });
  renderPanels();
  syncDock();
}

// ---------- opening: the Rust side looks at the file, the page reads it ----------
// a new workbook has no file yet: it opens through the same door, under a path of its own
const NEW = '\u0000new';
let pendingNames = [];
async function openPath(path) {
  if (path.startsWith(NEW))
    return pending && pending.path === NEW
      ? { status: 'ok', path: '', dir: '', name: path.slice(NEW.length), size: 0, kind: 'new' }
      : { status: 'abandoned' };
  pendingNames = [];
  const r = await own.openFile(path);
  if (r.status !== 'ok') return r;
  try {
    let sheets,
      info = null,
      ex = null,
      p = null,
      nm = {};
    if (r.kind === 'xlsx' || r.kind === 'xlsm') {
      const got = readXlsx(new Uint8Array(await own.sheetBytes()));
      sheets = got.sheets.map((s) => makeSheet(s.name, s.rows, s.widths, s.x));
      ex = got.extras;
      p = got.pkg;
      nm = got.names;
    } else if (MODELLED.includes(r.kind)) {
      epoch.base = 25569;
      const m = await own.sheetModel();
      // a table's or a database's first row is its field names (`header`); a database's large table is cut (`cut`: the
      // rows it has, sqlite.rs) and one with no tables opens as an empty sheet
      sheets = m.sheets.map((s) =>
        Object.assign(
          makeSheet(
            s.name,
            s.rows.map((row) => row.map((x) => (x == null ? '' : x)))
          ),
          s.header ? { headerOn: true } : {},
          s.cut ? { cut: s.cut } : {}
        )
      );
      if (!sheets.length) sheets = [makeSheet(T('工作表 1'), [])];
      // ODS's defined names, already in this page's notation; they go into a workbook saved from it
      nm = Object.fromEntries((m.names || []).map(([n, ref]) => [n.toUpperCase(), ref]));
      pendingNames = m.names || [];
    } else {
      epoch.base = 25569;
      const d = await own.sheetText(null);
      const delim = r.kind === 'tsv' ? '\t' : sniffDelimiter(d.text, r.name),
        parsed = parseDelimited(d.text, delim);
      sheets = [makeSheet(r.name.replace(/\.[^.]+$/, ''), parsed.rows, {}, null, parsed.raw)];
      info = { enc: d.enc, d: delim, eol: lineEndOf(d.text), bom: d.bom, finalEol: /[\r\n]$/.test(d.text) };
    }
    pending = { path: r.path, sheets, info, ex, pkg: p, names: nm, kind: r.kind, readonly: r.readonly, size: r.size };
    return r;
  } catch {
    own.sheetFailed();
    return { status: 'damaged' };
  }
}
function holdPending(r) {
  const p = pending;
  pending = null;
  hold(p.sheets, p.size, p.kind, p.info, p.ex, p.pkg, p.names, p.readonly);
  own.sheetOpened(r.path);
}
function hold(sheets, size, k, info, ex, p, nm, ro) {
  closeFind();
  shell.closePop();
  if (editing) {
    editing = null;
    editBox.remove();
  }
  book = sheets;
  at = 0;
  fileSize = size;
  kind = k;
  origin = k;
  csvInfo = info;
  extras = ex;
  pkg = p;
  names = nm || {};
  nameList = pendingNames;
  pendingNames = [];
  readOnly = !!ro;
  opened = true;
  cache = new Map();
  treeCache.clear();
  history = [{}];
  hAt = 0;
  savedAt = 0;
  moveSeq = 0;
  book.forEach((s) => {
    s.sel = { r: s.headerOn ? 1 : 0, c: 0, r2: s.headerOn ? 1 : 0, c2: 0 };
  });
  sel = sheet().sel;
  book.forEach(autoWidths);
  stage.scrollTop = 0;
  stage.scrollLeft = 0;
  drawTabs();
  syncPads();
  layout();
  shell.changed();
  showReadOnly(readOnly);
  stage.focus({ preventScroll: true });
  if (!reduce) gridEl.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}
function closeBook() {
  opened = false;
  book = [];
  pending = null;
  pkg = null;
  csvInfo = null;
  extras = null;
  readOnly = false;
  closeFind();
  shell.closePop();
  showReadOnly(false);
  tabSeg.hidden = true;
  rowsEl.textContent = '';
  frozenEl.textContent = '';
  drawn = new Map();
  root.classList.remove('many', 'plain-cell', 'viewing', 'has-frozen');
}
// New Spreadsheet: the family's second button, its menu item and {Mod+N} (the family asks about unsaved work first)
async function makeBook() {
  await own.sheetNew();
  pending = {
    path: NEW,
    sheets: [makeSheet(T('工作表 1'), [])],
    info: null,
    ex: null,
    pkg: null,
    names: {},
    kind: 'new',
    readonly: false,
    size: 0,
  };
  await shell.openPath(NEW + T('未命名') + '.xlsx');
}
const newBook = () => shell.guard(makeBook);
// ---------- keys ----------
function keys(e) {
  const mod = modKey(e),
    k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName);
  if (!opened) return;
  if (mod && k.toLowerCase() === 'f') {
    e.preventDefault();
    openFind();
    return;
  }
  if (k === 'Escape' && !typing) {
    if (closeFind()) return;
    if (!isOne()) select(sel.r, sel.c);
    return;
  }
  if (
    typing
    || document.querySelector('.ask-wrap.on')
    || document.querySelector('.dlg.on')
    || document.querySelector('.set.on')
  )
    return;
  if (mod && k.toLowerCase() === 'z') {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
    return;
  }
  if (mod && k.toLowerCase() === 'y' && yRedoes) {
    e.preventDefault();
    redo();
    return;
  }
  if (mod && k.toLowerCase() === 'a') {
    e.preventDefault();
    select(0, 0, displayCount() - 1, shownCols(sheet()) - 1, true, false);
    return;
  }
  if (k === ' ' && (mod || e.shiftKey)) {
    e.preventDefault();
    const b = box();
    if (e.shiftKey && !mod) select(b.r1, 0, b.r2, shownCols(sheet()) - 1, true, false);
    else select(0, b.c1, displayCount() - 1, b.c2, true, false);
    return;
  }
  if (mod && k.toLowerCase() === 'd') {
    e.preventDefault();
    fillShort(true);
    return;
  }
  if (mod && k.toLowerCase() === 'r') {
    e.preventDefault();
    fillShort(false);
    return;
  }
  if (e.altKey && (k === '=' || e.code === 'Equal')) {
    e.preventDefault();
    autoSum();
    return;
  }
  const dr = k === 'ArrowDown' ? 1 : k === 'ArrowUp' ? -1 : k === 'Enter' ? (e.shiftKey ? -1 : 1) : 0,
    dc = k === 'ArrowRight' ? 1 : k === 'ArrowLeft' ? -1 : k === 'Tab' ? (e.shiftKey ? -1 : 1) : 0;
  if (mod && (dr || dc) && k !== 'Enter' && k !== 'Tab') {
    e.preventDefault();
    const from = e.shiftKey ? [sel.r2, sel.c2] : [sel.r, sel.c],
      to = edgeFrom(from[0], from[1], dr, dc);
    if (e.shiftKey) select(sel.r, sel.c, to[0], to[1]);
    else select(to[0], to[1]);
    return;
  }
  if (dr || dc) {
    e.preventDefault();
    if (e.shiftKey && !['Enter', 'Tab'].includes(k)) select(sel.r, sel.c, sel.r2 + dr, sel.c2 + dc);
    else select(sel.r + dr, sel.c + dc);
    return;
  }
  if (k === 'PageDown' || k === 'PageUp') {
    e.preventDefault();
    const n = Math.floor(stage.clientHeight / RH) - 4;
    select(sel.r + (k === 'PageDown' ? n : -n), sel.c);
    return;
  }
  if (k === 'Home') {
    e.preventDefault();
    select(mod ? 0 : sel.r, 0);
    return;
  }
  if (k === 'End') {
    e.preventDefault();
    select(mod ? displayCount() - 1 : sel.r, colCount(sheet()) - 1);
    return;
  }
  if (k === 'Delete' || k === 'Backspace') {
    e.preventDefault();
    clearCells();
    return;
  }
  if (k === 'F2') {
    e.preventDefault();
    startEdit();
    return;
  }
  // (⌃ and a letter is the Mac's own, not a letter typed; on Windows Ctrl is the shortcut key and already out)
  if (k.length === 1 && !mod && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    startEdit(k);
  }
}
barInput.addEventListener('focus', () => {
  if (!isOne()) select(sel.r, sel.c);
});
barInput.addEventListener('input', fitBar);

// ---------- language ----------
shell.onLang(() => {
  fq.i.placeholder = T(fq.i.dataset.ph);
  fr.i.placeholder = T(fr.i.dataset.ph);
  // a new workbook not saved yet: its sheet keeps the name the program gave it in the language on screen (the family
  // renames the file itself), unless a formula already uses that name
  const s0 = opened && shell.doc && !shell.doc.path && book.length === 1 ? book[0] : null;
  if (
    s0
    && /^(工作表 1|Sheet 1)$/.test(s0.name)
    && s0.name !== T('工作表 1')
    && !s0.rows.some((r) => r && r.some((x) => x && typeof x === 'object' && x.f != null && x.f.includes(s0.name)))
  )
    s0.name = T('工作表 1');
  if (opened) {
    paint(true);
    syncDock();
    renderPanels();
    placeTabs();
  }
});
if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => placeTabs());

// ---------- begin ----------
stage.tabIndex = -1;
shell.test.sheets = {
  get book() {
    return book;
  },
  get sel() {
    return sel;
  },
  get at() {
    return at;
  },
  get kind() {
    return kind;
  },
  get origin() {
    return origin;
  },
  get readOnly() {
    return readOnly;
  },
  get csv() {
    return csvInfo;
  },
  get extras() {
    return extras;
  },
  textOf: (r, c, s = at) => shownText(textOf(book[s], r, c)),
  // a formula worked out here, not the result the file came with: { v } or { error } or { notOurs }
  compute(s, r, c) {
    const x = cellRaw(book[s], r, c);
    if (!x || x.f == null) return { constant: true };
    try {
      return {
        v: evaluate(parse(x.f), {
          read: (n, rr, cc) => {
            const t = n ? sheetNamed(n) : book[s];
            if (!t) throw new FormulaError('#REF!');
            return valueOf(t, rr, cc);
          },
          name: (n) => names[n.toUpperCase()] || null,
        }),
      };
    } catch (e) {
      return e instanceof NotSupported ? { notOurs: true } : { error: e.code || String(e) };
    }
  },
  view: () => dataRows(sheet()),
  select,
  switchSheet,
  insertRow,
  deleteRow,
  insertCol,
  deleteCol,
  fill,
  pasteText,
  undo,
  redo,
  clearView,
  newBook,
  setCell: (r, c, t) => {
    const s = sheet(),
      old = cellRaw(s, r, c);
    applyCells([{ s: at, r, c, old: old === undefined ? '' : old, now: fromTyped(t, old) }]);
  },
  sort: (c, up) => {
    sheet().sort = { c, up };
    viewChanged();
  },
  filter: (c, values) => {
    sheet().filters[c] = new Set(values);
    viewChanged();
  },
  textFor,
  get history() {
    return hAt;
  },
};
// with DOTANY_TEST set, shell.test.touch makes a change the way editing does (the first cell set to what it holds)
shell.test.touch = () => {
  if (!opened) return;
  const s = sheet(),
    old = cellRaw(s, 0, 0);
  applyCells([{ s: at, r: 0, c: 0, old: old === undefined ? '' : old, now: old === undefined ? '' : old }]);
};
shell.begin();
