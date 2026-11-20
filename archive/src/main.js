// DotAny Archive: the page of the program, inside the family's window. The list, the path after the file name, the dock
// that rises to show progress, the preview and info panels and the extract card are written here; the top bar, the
// save card, the file menu, settings, the close prompt and the empty window come from shared/web.
import { createShell, morph } from '@dotany/web/shell.js';
import * as be from '@dotany/web/backend.js';
import { el, root, reduce, icon, EASE } from '@dotany/web/shared.js';
import { T, size as sizeText } from '@dotany/web/i18n.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';
import { modKey, os } from '@dotany/web/keys.js';

// the tool's own words in other languages: src/lang/<code>.json (the family's are in shared/web/lang)
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
// the save card's formats; the extension is shown beside the name. RAR is never written.
const FORMATS = {
  zip: { ext: '.zip', label: 'ZIP' },
  '7z': { ext: '.7z', label: '7Z' },
  tgz: { ext: '.tar.gz', label: 'TAR.GZ' },
  tar: { ext: '.tar', label: 'TAR' },
};
const NEW = '\u0000new';
// every name Archive opens (src-tauri/src/app.rs, TOOL.extensions): one of these dropped on the empty window is opened,
// and a damaged one is refused, never packed into a new archive
const ARCHIVE = /\.(zip|7z|rar|tar|gz|tgz|bz2|tbz2|tbz|xz|txz|zst|tzst|lzh|lha|cpio|ar|deb|rpm)$/i;
const money = (b) => sizeText(b, { zero: true, gb: true });
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const MOVE = { duration: 240, easing: EASE },
  GROW = { duration: 420, easing: EASE };
const pad2 = (n) => String(n).padStart(2, '0');
const stamp = (d) =>
  `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
const fromB64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
const say = (k) => shell.say(T(k));
// words with a number in them; each language says its counts its own way (src/lang)
const N = {
  items: (n) => T('{n} 项', { n }),
  selected: (n) => T('已选 {n} 项', { n }),
  chosen: (n) => T('所选 {n} 项', { n }),
  selectedSize: (n, size) => T('已选 {n} 项 · {size}', { n, size }),
  files: (n, size) => T('{n} 个文件 · {size}', { n, size }),
  filesFolders: (f, d) => T('{f} 个文件，{d} 个文件夹', { f, d }),
  needPassword: (n) => T('{n} 个文件需要密码', { n }),
  more: () => T(' 等'),
  openIn: (who) => T('用 {who} 打开', { who }),
  damaged: (n) => T('{n} 个文件已损坏', { n }),
  skipped: (n) => T(' · 跳过 {n} 个', { n }),
};

// ---------- what's open: a flat list of paths; folders are entries too ----------
let opened = false,
  fileName = '',
  fileSize = 0,
  kindExt = '',
  nodes = [],
  cwd = '',
  picked = new Set(),
  anchor = null,
  sortKey = 'name',
  sortUp = true,
  encoding = 'utf-8',
  rawEntries = [],
  password = null,
  hasOdd = false,
  docDir = '';
let installed = [];
// what the folders hold, worked out once per change instead of once per row
let kids = new Map(),
  dirSize = new Map(),
  fileCount = 0;
const parentOf = (p) => {
  const s = p.replace(/\/$/, ''),
    i = s.lastIndexOf('/');
  return i < 0 ? '' : s.slice(0, i + 1);
};
const baseOf = (p) => {
  const s = p.replace(/\/$/, '');
  return s.slice(s.lastIndexOf('/') + 1);
};
function index() {
  kids = new Map();
  dirSize = new Map();
  fileCount = 0;
  for (const n of nodes) {
    const up = parentOf(n.path);
    if (!kids.has(up)) kids.set(up, []);
    kids.get(up).push(n);
    if (n.dir) continue;
    fileCount++;
    for (let p = up; p; p = parentOf(p)) dirSize.set(p, (dirSize.get(p) || 0) + n.size);
  }
}
const here = () => kids.get(cwd) || [];
const under = (dir) => nodes.filter((n) => !n.dir && n.path.startsWith(dir));
const sizeOf = (n) => (n.dir ? dirSize.get(n.path) || 0 : n.size);
const ICONS = {
  image: /\.(png|jpe?g|gif|webp|bmp|svg|ico|tiff?|avif|heic)$/i,
  code: /\.(js|ts|json|html?|css|py|rs|c|cpp|h|java|go|sh|xml|ya?ml|toml|ini)$/i,
  sheet: /\.(csv|tsv|xlsx?)$/i,
  text: /\.(txt|md|log|docx?|srt|vtt|epub|pdf|rtf)$/i,
  video: /\.(mp4|mkv|mov|avi|webm|wmv)$/i,
  music: /\.(mp3|flac|wav|aiff?|ogg|m4a)$/i,
  pack: /\.(zip|7z|tar|gz|tgz|rar|bz2|tbz2?|xz|txz|zst|tzst|lzh|lha|cpio|deb|rpm)$/i,
};
const kindOf = (name) => {
  for (const k in ICONS) if (ICONS[k].test(name)) return k;
  return 'doc';
};
const OPENER = [
  [/\.(png|jpe?g|gif|webp|bmp|svg|ico|tiff?)$/i, 'Image'],
  [/\.(txt|md|log|csv|tsv|json|js|ts|html?|css|py|xml|ya?ml|toml|ini|srt|vtt|docx)$/i, 'Text'],
  [/\.pdf$/i, 'PDF'],
  [/\.(mp4|mkv|mov|avi|webm|wmv)$/i, 'Video'],
  [/\.(mp3|flac|wav|aiff?|ogg|m4a)$/i, 'Audio'],
  [/\.(epub|fb2|cbz|djvu|mobi)$/i, 'Books'],
];
const toolFor = (name) => {
  const o = OPENER.find(([r]) => r.test(name));
  return o ? o[1] : null;
};
// a DotAny tool for this kind of file, when it is installed beside this one
const openerFor = (name) => {
  const t = toolFor(name);
  return t && installed.includes('dotany-' + t.toLowerCase()) ? 'DotAny ' + t : null;
};

// ---------- names: valid UTF-8 first, then whichever reading looks like real words ----------
function guessEncoding(entries) {
  const bytes = entries.filter((e) => e.raw).map((e) => e.raw);
  if (!bytes.length) return 'utf-8';
  const whole = new Uint8Array(bytes.reduce((n, a) => n + a.length + 1, 0));
  let at = 0;
  for (const a of bytes) {
    whole.set(a, at);
    at += a.length + 1;
    whole[at - 1] = 47;
  }
  try {
    new TextDecoder('utf-8', { fatal: true }).decode(whole);
    return 'utf-8';
  } catch {
    /* not UTF-8 */
  }
  let best = 'gbk',
    top = -Infinity;
  for (const enc of ['gbk', 'big5', 'shift_jis']) {
    try {
      const s = new TextDecoder(enc, { fatal: true }).decode(whole);
      const han = (s.match(/[一-龥]/g) || []).length,
        kana = (s.match(/[぀-ヿ]/g) || []).length,
        odd = (s.match(/[-｡-ﾟ]/g) || []).length;
      const common = (
        s.match(
          /[的一是不了在人有我他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对可她里后小么心多天而能好都然没日于起还发成事只作当想看文无开手十用主行方又如前所本见经头面公同三已老从动两长知民样现分将外但身些与高意进把法此实回二理美点月明其种声全工己话儿者向情部正名定女问力机给等几很业最间新什打便位因重被走电四第门相次东政海口使教西再平真听世气信北少关并内加化由却代军产入先山五太水万市眼体别处总才场师书比住员九笑性通目华报立马命张活难神数件安表原车白应路期叫死常提感金何更反合放做系计或司利受光王果亲界及今京务制解各任至清物台象记边共风战干接它许八特觉望直服毛林题建南度统色字请交爱让认算论百吃义科怎元社术结六功指思非流每青管夫连远资队跟带花快条院变联言权往展该领传近留红治决周保达办运武半候七必城父强步完革深区即求品士转量空甚众技轻程告江语英基派满式李息写呢识极令黄德收脸钱党倒未持取设始版双历越史商千片容研像找友孩站广改议形委早房音火际则首单据导影失拿网香似斯专石若兵弟谁校读志飞观争究包组造落视济喜离虽坐集编宝谈府拉黑且随格尽剑讲布杀微怕母调局根曾准团段终乐切级克精哪官示冷域读照片资料项目报告文件夹备份]/g
        ) || []
      ).length;
      const score = han * 0.2 + common * 2 + kana * 3 - odd * 4;
      if (score > top) {
        top = score;
        best = enc;
      }
    } catch {
      /* not this one */
    }
  }
  return best;
}
const decodeName = (e, enc) => (e.raw ? new TextDecoder(enc).decode(e.raw) : e.n);

// ---------- the window: the family's, with this tool's parts in it ----------
const shell = createShell({
  name: 'DotAny Archive',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽文件至此',
  fileKind: '压缩包',
  licences: [
    ['zip · sevenz-rust2 · tar · flate2 · bzip2 · lzma-rust2 · ruzstd', '归档引擎', 'MIT / Apache-2.0'],
    ['libarchive', 'RAR 解析组件', 'BSD-2-Clause'],
  ],
  formats: FORMATS,
  formatOf: () => (kindExt === '7z' ? '7z' : kindExt.startsWith('tar.') ? 'tgz' : kindExt === 'tar' ? 'tar' : 'zip'),
  cantOpen: { damaged: '文件已损坏，或并非受支持的压缩归档格式。' },
  failWords: {
    password: '未设置加密密码，未能完成保存。当前编辑仍完整保留在此窗口中。',
    damaged: '归档内存在损坏的数据块，未能完成保存。当前编辑仍完整保留在此窗口中。',
  },
  open: openArchive,
  show: (r) => hold(r),
  clear: () => clearAll(),
  dirty: () => opened && history[hAt] !== savedSnap,
  discard() {
    savedSnap = history[hAt];
  },
  estimate: (fmt) => own.estimateSave(fmt, levelKey, plainNodes(nodes)),
  saveRows: (box, fmt, estimate) => drawSaveRows(box, fmt, estimate),
  save: saveArchive,
  saved: (r) => afterSave(r),
  keys: onKey,
  // New Archive (新建压缩包) beside Open Local File, as Text has New File
  newFile: { label: '新建压缩包', run: () => shell.openPath(NEW) },
  // into what's open, dropped files are added; on the empty window one archive opens, anything else makes a new archive of it
  drop: async (paths) => {
    if (!opened && paths.length === 1 && ARCHIVE.test(paths[0])) {
      shell.openRequest(paths[0]);
      return;
    }
    const items = own.statPaths(paths);
    if (!opened) await shell.openPath(NEW);
    if (opened) addItems(await items);
  },
  dropLabel: () => (opened ? '松开以添加文件' : '松开以打开文件'),
});
const { app, stage, head } = shell;
const where = head.querySelector('.where'),
  stateEl = head.querySelector('.state'),
  metaEl = shell.metaEl;
const sound = () => shell.sound();
const crumbs = el('nav', 'crumbs');
where.insertBefore(crumbs, stateEl);
const countEl = el('span', 'count dyn');
metaEl.insertBefore(countEl, metaEl.firstChild);
const preview = shell.sidePanel('preview', 'eye', '预览'),
  previewBtn = document.getElementById('previewBtn');
previewBtn.dataset.tip = '预览 · 空格';
const findBtn = shell.headButton('findBtn', 'search', '查找', '查找 · {Mod+F}');
findBtn.setAttribute('aria-pressed', 'false');
const info = shell.sidePanel('info', 'info', '信息'),
  infoBtn = document.getElementById('infoBtn');
[preview, info].forEach((p) => p.classList.add('right'));

const listEl = el('div', 'ar-list'),
  headRow = el('div', 'ar-head'),
  rowsEl = el('div', 'ar-rows');
rowsEl.id = 'rows';
listEl.append(headRow, rowsEl);
stage.appendChild(listEl);

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
const upBtn = barBtn('upBtn', 'back', '返回上一级', () => goUp());
upBtn.classList.add('ar-up');
const addBtn = barBtn('addBtn', 'add', '添加文件', () => pickAndAdd()),
  dirBtn = barBtn('dirBtn', 'folderPlus', '新建文件夹', () => newFolder()),
  renameBtn = barBtn('renameBtn', 'rename', '重命名', () => startRename()),
  delBtn = barBtn('delBtn', 'trash', '删除', () => removePicked());
const undoBtn = barBtn('undo', 'undo', '撤销 · {Mod+Z}', () => undo()),
  redoBtn = barBtn('redo', 'redo', '重做 · {Mod+Y}', () => redo());
const pickInfo = el('span', 'ar-count dyn');
pickInfo.id = 'pickInfo';
const extractBtn = el('button', 'btn primary ar-go dyn', '解压全部');
extractBtn.type = 'button';
extractBtn.id = 'extractBtn';
extractBtn.addEventListener('mousedown', (e) => e.preventDefault());
extractBtn.addEventListener('click', () => openExtract());
const group = (items) => {
  const g = el('div', 'grp');
  g.append(...items);
  return g;
};
const pill = el('div', 'pill ar-pill'),
  pillRow = el('div', 'pill-row');
pillRow.append(
  pickInfo,
  el('i', 'sep'),
  group([addBtn, dirBtn, renameBtn, delBtn]),
  el('i', 'sep'),
  group([undoBtn, redoBtn]),
  el('i', 'sep'),
  extractBtn
);
// extracting: the dock rises and carries the line, the way DotAny Video's rises to trim
const prog = el('div', 'ar-prog'),
  progTop = el('div', 'ar-prog-top'),
  progName = el('span', 'ar-prog-name dyn'),
  progPct = el('span', 'ar-prog-pct dyn'),
  progLine = el('div', 'ar-line'),
  progFill = el('i', 'ar-fill'),
  progNow = el('i', 'ar-now'),
  progBtns = el('div', 'ar-prog-btns');
progLine.append(progFill, progNow);
progTop.append(progName, progPct, progBtns);
prog.append(progTop, progLine);
prog.id = 'prog';
pill.append(prog, pillRow);
shell.place(pill);
shell.translates(pill, upBtn);

// ---------- the list ----------
const gutter = () => stage.offsetWidth - stage.clientWidth;
const panelOpen = () => preview.classList.contains('on') || info.classList.contains('on');
function syncPads() {
  root.style.setProperty('--pad-r', (panelOpen() ? Math.max(0, 280 - gutter()) : 0) + 'px');
  root.style.setProperty('--gut', gutter() + 'px');
}
const collator = new Intl.Collator('zh-CN', { numeric: true });
const sorted = (a) =>
  a.slice().sort((x, y) => {
    if (x.dir !== y.dir) return x.dir ? -1 : 1;
    const d =
      sortKey === 'size'
        ? sizeOf(x) - sizeOf(y)
        : sortKey === 'time'
          ? (x.time || '').localeCompare(y.time || '')
          : collator.compare(baseOf(x.path), baseOf(y.path));
    return sortUp ? d : -d;
  });
const heads = [
  ['name', '名称', 'c-name'],
  ['size', '大小', 'c-size'],
  ['time', '修改时间', 'c-time'],
];
function drawHead() {
  headRow.textContent = '';
  headRow.appendChild(upBtn);
  heads.forEach(([k, t, c]) => {
    const b = el('button', 'ar-sort ' + c + ' dyn');
    b.type = 'button';
    b.id = 'sort-' + k;
    b.innerHTML = `<span>${T(t)}</span>` + (sortKey === k ? icon(sortUp ? 'up14' : 'chevron') : '');
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      if (sortKey === k) sortUp = !sortUp;
      else {
        sortKey = k;
        sortUp = k === 'name';
      }
      drawHead();
      drawRows();
    });
    headRow.appendChild(b);
  });
}
// rows are 36 high, so a long folder draws only what's in view, with room above and below
const ROW = 36,
  SLACK = 24,
  MANY = 400;
let shown = [],
  searching = false,
  big = 1,
  first = 0,
  last = 0;
function rowFor(n) {
  const row = el('div', 'ar-row');
  row.dataset.path = n.path;
  row.setAttribute('role', 'row');
  row.classList.toggle('on', picked.has(n.path));
  const ic = n.dir ? 'folder' : kindOf(n.path),
    nm = el('span', 'c-name ar-nm'),
    label = el('span', 'ar-text', baseOf(n.path));
  nm.innerHTML = icon(ic);
  nm.appendChild(label);
  if (n.lock) {
    const l = el('span', 'ar-lock');
    l.innerHTML = icon('lock');
    nm.appendChild(l);
  }
  if (searching && parentOf(n.path)) nm.appendChild(el('span', 'ar-in', parentOf(n.path).replace(/\/$/, '')));
  const sz = el('span', 'c-size ar-sz'),
    s = sizeOf(n),
    bar = el('i', 'ar-bar'),
    fill = el('i', 'ar-bar-in');
  fill.style.width = (s / big) * 100 + '%';
  bar.appendChild(fill);
  sz.append(bar, el('span', 'ar-sz-t', n.dir && !s ? '—' : money(s)));
  row.append(nm, sz, el('span', 'c-time ar-tm', n.time || ''));
  return row;
}
const topSpace = el('div', 'ar-space'),
  endSpace = el('div', 'ar-space');
function paintRows(force) {
  const many = shown.length > MANY;
  let a = 0,
    b = shown.length;
  if (many) {
    const y = stage.scrollTop - (rowsEl.offsetTop || 0),
      h = stage.clientHeight;
    a = clamp(Math.floor(y / ROW) - SLACK, 0, shown.length);
    b = clamp(Math.ceil((y + h) / ROW) + SLACK, 0, shown.length);
    if (!force && a === first && b === last) return;
  }
  first = a;
  last = b;
  rowsEl.textContent = '';
  if (many) {
    topSpace.style.height = a * ROW + 'px';
    rowsEl.appendChild(topSpace);
  }
  for (let k = a; k < b; k++) rowsEl.appendChild(rowFor(shown[k]));
  if (many) {
    endSpace.style.height = (shown.length - b) * ROW + 'px';
    rowsEl.appendChild(endSpace);
  }
}
stage.addEventListener(
  'scroll',
  () => {
    if (shown.length > MANY) {
      paintRows(false);
      if (renaming && !renaming.i.isConnected) finishRename(true);
    }
  },
  { passive: true }
);
function drawRows(dir = 0) {
  const q = findIn.value.trim().toLowerCase();
  searching = findBox.classList.contains('on') && !!q;
  shown = searching ? sorted(nodes.filter((n) => baseOf(n.path).toLowerCase().includes(q))) : sorted(here());
  big = Math.max(1, ...shown.slice(0, 5000).map(sizeOf));
  paintRows(true);
  if (!shown.length)
    rowsEl.appendChild(
      el('p', 'ar-none dyn', searching ? T('无匹配结果') : nodes.length ? T('此文件夹为空。') : T('拖拽文件至此'))
    );
  rowsEl.classList.toggle('blank', !nodes.length && !searching);
  if (dir && !reduce)
    rowsEl.animate(
      [
        { opacity: 0, transform: `translateX(${16 * dir}px)` },
        { opacity: 1, transform: 'none' },
      ],
      MOVE
    );
  syncAll();
}
function drawCrumbs() {
  crumbs.textContent = '';
  if (!cwd) return;
  const parts = cwd.replace(/\/$/, '').split('/');
  parts.forEach((p, i) => {
    crumbs.appendChild(el('i', 'crumb-sep', '/'));
    const b = el('button', 'crumb', p);
    b.type = 'button';
    b.dataset.to = parts.slice(0, i + 1).join('/') + '/';
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      go(b.dataset.to, -1);
    });
    crumbs.appendChild(b);
  });
}
function go(dir, way) {
  if (renaming) finishRename(true);
  cwd = dir;
  picked = new Set();
  anchor = null;
  stage.scrollTop = 0;
  drawCrumbs();
  drawRows(way);
  renderPanel();
}
function goUp() {
  if (cwd) go(parentOf(cwd), -1);
}
function syncAll() {
  [...rowsEl.children].forEach((r) => {
    if (r.dataset.path) r.classList.toggle('on', picked.has(r.dataset.path));
  });
  const sel = nodes.filter((n) => picked.has(n.path)),
    bytes = sel.reduce((a, n) => a + sizeOf(n), 0);
  pickInfo.textContent = sel.length ? N.selectedSize(sel.length, money(bytes)) : N.items(shown.length);
  extractBtn.textContent = T(sel.length ? '解压所选' : '解压全部');
  extractBtn.disabled = !fileCount || root.classList.contains('extracting');
  root.classList.toggle('at-root', !cwd);
  renameBtn.disabled = sel.length !== 1;
  delBtn.disabled = !sel.length;
  undoBtn.disabled = hAt <= 0;
  redoBtn.disabled = hAt >= history.length - 1;
  countEl.textContent = N.files(fileCount, money(fileSize));
}
// clicking: one picks it, Ctrl adds, Shift runs; twice goes in
rowsEl.addEventListener('click', (e) => {
  if (renaming && e.target.closest('.ar-rename')) return;
  const r = e.target.closest('.ar-row');
  if (!r) {
    if (picked.size) {
      picked = new Set();
      anchor = null;
      syncAll();
      renderPanel();
    }
    return;
  }
  const p = r.dataset.path;
  if (e.shiftKey && anchor) {
    const list = shown.map((n) => n.path),
      a = list.indexOf(anchor),
      b = list.indexOf(p);
    picked = new Set(list.slice(Math.min(a, b), Math.max(a, b) + 1));
  } else if (modKey(e)) {
    if (picked.has(p)) picked.delete(p);
    else picked.add(p);
    anchor = p;
  } else {
    picked = new Set([p]);
    anchor = p;
  }
  syncAll();
  renderPanel();
});
rowsEl.addEventListener('mousedown', (e) => {
  if (e.detail > 1 || e.shiftKey) e.preventDefault();
});
stage.addEventListener('click', (e) => {
  if (e.target === stage || e.target === listEl) {
    if (picked.size) {
      picked = new Set();
      syncAll();
      renderPanel();
    }
  }
});
rowsEl.addEventListener('dblclick', (e) => {
  const r = e.target.closest('.ar-row');
  if (r) openNode(nodes.find((n) => n.path === r.dataset.path));
});
async function openNode(n) {
  if (!n) return;
  if (n.dir) {
    sound();
    if (findBox.classList.contains('on')) closeFind();
    go(n.path, 1);
    return;
  }
  if (n.lock && kindExt === 'rar') {
    sound();
    say('暂不支持打开加密的 RAR 压缩包。');
    return;
  }
  if (n.lock && password == null) {
    try {
      await askPassword(false);
    } catch {
      return;
    }
  }
  const t = toolFor(n.path),
    r = await own.openEntry({
      i: n.add ? null : n.src.i,
      add: n.add || null,
      name: baseOf(n.path),
      tool: t ? 'dotany-' + t.toLowerCase() : null,
      password,
    });
  if (r === 'refused') {
    sound();
    say('暂不支持该文件格式。');
  } else if (r === 'unsupported') {
    sound();
    say('暂不支持打开加密的 RAR 压缩包。');
  } else if (r === 'damaged') {
    sound();
    say('文件已损坏，或并非受支持的压缩归档格式。');
  }
}

// ---------- undo: one list for everything that changes the archive ----------
let history = [],
  hAt = 0,
  savedSnap = null;
const snap = () => nodes.slice();
function commit() {
  history = history.slice(0, hAt + 1);
  history.push(snap());
  hAt = history.length - 1;
  index();
  syncAll();
  shell.changed();
}
function restore(s) {
  nodes = s.slice();
  index();
  picked = new Set();
  if (cwd && !nodes.some((n) => n.path === cwd)) {
    cwd = '';
    drawCrumbs();
  }
  drawRows();
  renderPanel();
  shell.changed();
}
function undo() {
  if (hAt > 0) {
    hAt--;
    restore(history[hAt]);
  }
}
function redo() {
  if (hAt < history.length - 1) {
    hAt++;
    restore(history[hAt]);
  }
}

// ---------- changing what's inside ----------
function removePicked() {
  if (!picked.size) return;
  const gone = [...picked],
    rows = [...rowsEl.children].filter((r) => picked.has(r.dataset.path));
  const finish = () => {
    nodes = nodes.filter((n) => !gone.some((g) => n.path === g || (g.endsWith('/') && n.path.startsWith(g))));
    picked = new Set();
    index();
    drawRows();
    renderPanel();
    commit();
  };
  if (reduce || !rows.length) {
    finish();
    return;
  }
  const fades = rows.map((r) =>
    r.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 140, easing: EASE, fill: 'forwards' })
  );
  fades[0].onfinish = finish;
}
function uniquePath(dir, name, isDir) {
  const dot_ = isDir ? -1 : name.lastIndexOf('.'),
    stem = dot_ > 0 ? name.slice(0, dot_) : name,
    ext = dot_ > 0 ? name.slice(dot_) : '';
  let i = 1,
    n = name;
  while (nodes.some((x) => x.path === dir + n + (isDir ? '/' : ''))) n = `${stem} (${++i})${ext}`;
  return dir + n + (isDir ? '/' : '');
}
function newFolder() {
  const path = uniquePath(cwd, T('新建文件夹'), true);
  nodes = [...nodes, { path, dir: true, size: 0, time: stamp(new Date()), u: Math.floor(Date.now() / 1000) }];
  picked = new Set([path]);
  anchor = path;
  index();
  drawRows();
  commit();
  startRename(path, true);
}
// files and folders from disk: each dropped item keeps its own name, made unique where it lands
function addItems(items) {
  if (!items.length) return;
  const tops = new Map(),
    added = [];
  for (const it of items) {
    const [top, ...rest] = it.rel.split('/');
    if (!tops.has(top)) tops.set(top, uniquePath(cwd, top, it.dir || rest.length > 0).replace(/\/$/, ''));
    const path = [tops.get(top), ...rest].join('/') + (it.dir ? '/' : '');
    const when = it.u ? new Date(it.u * 1000) : new Date();
    added.push({ path, dir: it.dir, size: it.size, time: stamp(when), u: it.u, add: it.dir ? null : it.from });
  }
  nodes = [...nodes, ...added];
  picked = new Set(
    [...tops.values()]
      .map((t) => (added.some((n) => n.path === t + '/') ? t + '/' : t))
      .filter((p) => nodes.some((n) => n.path === p))
  );
  index();
  drawRows();
  renderPanel();
  commit();
  if (!reduce)
    [...rowsEl.children]
      .filter((r) => picked.has(r.dataset.path))
      .forEach((r) => r.animate([{ opacity: 0 }, { opacity: 1 }], MOVE));
}
async function pickAndAdd() {
  const paths = await own.pickFiles(docDir || null);
  if (paths.length) addItems(await own.statPaths(paths));
}
// renaming: the family's caret, right in the row
let renaming = null;
function startRename(path, fresh) {
  path = path || [...picked][0];
  if (!path) return;
  const r = [...rowsEl.children].find((x) => x.dataset.path === path);
  if (!r) return;
  const n = nodes.find((x) => x.path === path),
    box = el('span', 'fi ar-rename'),
    i = document.createElement('input');
  i.type = 'text';
  i.id = 'renameInput';
  i.spellcheck = false;
  i.autocomplete = 'off';
  i.value = baseOf(path);
  box.appendChild(i);
  const label = r.querySelector('.ar-text');
  label.replaceWith(box);
  inkfall(i);
  renaming = { path, i, n, fresh };
  i.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      finishRename();
    } else if (e.key === 'Escape') finishRename(true);
  });
  i.addEventListener('blur', () =>
    setTimeout(() => {
      if (renaming && renaming.i === i) finishRename();
    }, 0)
  );
  setTimeout(() => {
    i.focus();
    const stop = !n.dir && i.value.lastIndexOf('.') > 0 ? i.value.lastIndexOf('.') : i.value.length;
    i.setSelectionRange(0, stop);
  }, 20);
}
function finishRename(cancel) {
  const r = renaming;
  if (!r) return;
  renaming = null;
  const v = r.i.value.trim(),
    oldName = baseOf(r.path);
  if (cancel || !v || v === oldName || /[\\/:*?"<>|]/.test(v) || v === '.' || v === '..') {
    drawRows();
    return;
  }
  const to = parentOf(r.path) + v + (r.n.dir ? '/' : '');
  if (nodes.some((x) => x.path === to)) {
    say('当前目录下已存在同名文件夹。');
    drawRows();
    return;
  }
  nodes = nodes.map((x) =>
    x.path === r.path || (r.n.dir && x.path.startsWith(r.path)) ? { ...x, path: to + x.path.slice(r.path.length) } : x
  );
  picked = new Set([to]);
  anchor = to;
  index();
  drawRows();
  renderPanel();
  commit();
}

// ---------- the password: asked once, only when it's needed ----------
const pwWrap = el('div', 'ask-wrap');
pwWrap.setAttribute('role', 'alertdialog');
pwWrap.setAttribute('aria-modal', 'true');
const pwBox = el('div', 'ask ar-ask'),
  pwTitle = el('p', 'ask-title dyn'),
  pwWhich = el('p', 'side-note ar-which dyn'),
  pwField = el('label', 'field ar-pw'),
  pwIn = el('span', 'field-in'),
  pwInput = document.createElement('input'),
  pwEye = el('button', 'f-b'),
  pwNote = el('p', 'warn-t ar-pw-note dyn'),
  pwRow = el('div', 'ask-row');
pwInput.type = 'password';
pwInput.id = 'pwInput';
pwInput.autocomplete = 'off';
pwInput.spellcheck = false;
pwInput.setAttribute('aria-label', '密码');
pwIn.appendChild(pwInput);
pwEye.type = 'button';
pwEye.id = 'pwEye';
pwEye.innerHTML = icon('eye');
pwEye.setAttribute('aria-label', '显示密码');
pwEye.addEventListener('mousedown', (e) => e.preventDefault());
pwEye.addEventListener('click', (e) => {
  e.preventDefault();
  sound();
  const show = pwInput.type === 'password';
  pwInput.type = show ? 'text' : 'password';
  pwEye.innerHTML = icon(show ? 'eyeOff' : 'eye');
});
pwField.append(pwIn, pwEye);
const pwCancel = el('button', 'btn soft dyn', '取消'),
  pwOk = el('button', 'btn primary', '打开');
pwCancel.type = pwOk.type = 'button';
pwCancel.id = 'pwCancel';
pwOk.id = 'pwOk';
pwRow.append(pwNote, el('i', 'ask-gap'), pwCancel, pwOk);
pwBox.append(pwTitle, pwWhich, pwField, pwRow);
pwWrap.appendChild(pwBox);
document.body.appendChild(pwWrap);
shell.translates(pwWrap);
let pwDone = null;
// `test` answers whether a password is right: for an archive whose names are locked, or for one locked file
function askPassword(canSkip, whole = null) {
  return new Promise((ok, no) => {
    const locked = nodes.filter((n) => n.lock),
      names = locked
        .slice(0, 2)
        .map((n) => baseOf(n.path))
        .join(T('、'));
    // an empty file decrypts to nothing and proves nothing: the smallest one with something in it is read
    const probe = locked.filter((n) => n.size > 0).sort((x, y) => x.size - y.size)[0] || locked[0];
    pwDone = { ok, no, test: whole ? whole.test : (v) => own.tryPassword(probe.src.i, v) };
    pwTitle.textContent = whole ? T('此压缩包受密码保护') : N.needPassword(locked.length);
    pwWhich.textContent = whole ? whole.name : names + (locked.length > 2 ? N.more() : '');
    pwCancel.textContent = T(canSkip ? '跳过异常文件' : '取消');
    pwInput.placeholder = T('输入密码');
    pwInput.value = '';
    pwInput.type = 'password';
    pwEye.innerHTML = icon('eye');
    pwNote.textContent = '';
    pwField.classList.remove('bad');
    pwWrap.classList.add('on');
    setTimeout(() => pwInput.focus(), 30);
  });
}
let trying = false;
async function tryPassword() {
  if (!pwDone || trying) return;
  const v = pwInput.value;
  trying = true;
  const answer = await pwDone.test(v);
  trying = false;
  if (answer !== 'ok') {
    sound();
    pwField.classList.add('bad');
    pwNote.textContent = T(answer === 'unsupported' ? '暂不支持打开加密的 RAR 压缩包。' : '密码不正确，请重试。');
    pwNote.classList.remove('swap');
    void pwNote.offsetWidth;
    pwNote.classList.add('swap');
    pwInput.focus();
    pwInput.select();
    return;
  }
  sound();
  pwWrap.classList.remove('on');
  const d = pwDone;
  pwDone = null;
  if (d) d.ok(v);
}
function cancelPassword() {
  sound();
  pwWrap.classList.remove('on');
  const d = pwDone;
  pwDone = null;
  if (d) d.no(new Error('cancelled'));
}
pwOk.addEventListener('click', tryPassword);
pwCancel.addEventListener('click', cancelPassword);
pwInput.addEventListener('input', () => {
  if (pwField.classList.contains('bad')) {
    pwField.classList.remove('bad');
    pwNote.textContent = '';
  }
});
pwInput.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    e.preventDefault();
    tryPassword();
  } else if (e.key === 'Escape') cancelPassword();
});
// a locked file's password, once: the password every later read uses
async function needPassword(canSkip) {
  if (password != null || !nodes.some((n) => n.lock)) return true;
  if (kindExt === 'rar') return false;
  try {
    password = await askPassword(canSkip);
    return true;
  } catch {
    return false;
  }
}

// ---------- the panels, laid out as DotAny Image's. Preview follows the selection ----------
let previewToken = 0;
function renderPanel() {
  syncPads();
  if (!opened) return;
  if (preview.classList.contains('on')) {
    preview.textContent = '';
    const body = el('div', 'side-body');
    preview.append(el('h3', 'dyn', T('预览')), body);
    panelPreview(body);
  }
  if (info.classList.contains('on')) {
    info.textContent = '';
    const body = el('div', 'side-body');
    info.append(el('h3', 'dyn', T('信息')), body);
    panelInfo(body);
  }
}
const factRow = (dl, k, v) => {
  if (!v) return;
  const d = el('div');
  d.append(el('dt', 'dyn', T(k)), el('dd', 'dyn', v));
  dl.appendChild(d);
};
function factsOf(n) {
  const dl = el('dl', 'info-list');
  factRow(dl, '大小', money(sizeOf(n)));
  if (!n.dir && n.src) factRow(dl, '压缩后大小', money(n.src.c));
  factRow(dl, '修改时间', n.time);
  return dl;
}
const scopeFiles = () =>
  nodes.filter((n) => !n.dir && [...picked].some((p) => n.path === p || (p.endsWith('/') && n.path.startsWith(p))));
const packedOf = (list) => list.reduce((a, n) => a + (n.src ? n.src.c : 0), 0);
function panelPreview(body) {
  const token = ++previewToken,
    sel = nodes.filter((n) => picked.has(n.path));
  if (sel.length !== 1) {
    const list = sel.length ? scopeFiles() : under(cwd),
      rows_ = sel.length ? sel : here(),
      dl = el('dl', 'info-list');
    body.appendChild(el('p', 'ar-pv-name', sel.length ? N.selected(sel.length) : cwd ? baseOf(cwd) : fileName));
    factRow(dl, '包含项目', String(rows_.length));
    factRow(dl, '大小', money(list.reduce((a, n) => a + n.size, 0)));
    factRow(dl, '压缩后大小', money(packedOf(list)));
    body.appendChild(dl);
    return;
  }
  const n = sel[0],
    show = el('div', 'ar-show');
  body.appendChild(el('p', 'ar-pv-name', baseOf(n.path)));
  if (!n.dir) body.appendChild(show);
  body.appendChild(factsOf(n));
  const who = !n.dir && openerFor(n.path);
  if (who) {
    const b = el('button', 'btn soft ar-open dyn', N.openIn(who));
    b.type = 'button';
    b.id = 'openIn';
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => openNode(n));
    body.appendChild(b);
  }
  if (n.dir) return;
  const kind = kindOf(n.path);
  if ((kind !== 'image' && kind !== 'text' && kind !== 'code' && kind !== 'sheet') || (n.lock && kindExt === 'rar')) {
    show.remove();
    return;
  }
  (async () => {
    let b;
    try {
      if (n.lock && password == null && !(await needPassword(false))) throw new Error('locked');
      if (token !== previewToken) return;
      const limit = kind === 'image' ? 24 << 20 : 12000;
      b = new Uint8Array(n.add ? await own.readLocal(n.add, limit) : await own.readEntry(n.src.i, password, limit));
    } catch {
      if (token === previewToken) show.remove();
      return;
    }
    if (token !== previewToken) return;
    if (kind === 'image') {
      const i = el('img', 'ar-pic');
      i.alt = '';
      i.src = URL.createObjectURL(new Blob([b]));
      i.addEventListener('error', () => show.remove());
      show.appendChild(i);
    } else {
      const pre = el('pre', 'ar-pre');
      let s;
      try {
        s = new TextDecoder('utf-8', { fatal: true }).decode(b.subarray(0, 12000));
      } catch {
        s = new TextDecoder(encoding === 'utf-8' ? 'gbk' : encoding).decode(b.subarray(0, 12000));
      }
      pre.textContent = s;
      show.appendChild(pre);
    }
    if (!reduce) show.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  })();
}
let checking = false,
  checkBad = null; // the last check's damaged count, worded when drawn so it follows the language
const checkWord = () =>
  checkBad == null ? T('校验完整性') : checkBad ? N.damaged(checkBad) : T('校验通过，归档文件完整无损');
function panelInfo(body) {
  const dl = el('dl', 'info-list');
  const files = under(''),
    orig = files.reduce((a, n) => a + n.size, 0),
    comp = packedOf(files);
  factRow(dl, '压缩格式', kindExt.toUpperCase());
  factRow(dl, '包含项目', N.filesFolders(files.length, nodes.length - files.length));
  factRow(dl, '原始大小', money(orig));
  factRow(dl, '压缩后大小', money(comp));
  if (orig && comp) factRow(dl, '空间节省', Math.max(0, Math.round((1 - comp / orig) * 100)) + '%');
  if (nodes.some((n) => n.lock)) factRow(dl, '密码保护', T('已加密'));
  body.appendChild(dl);
  if (hasOdd) {
    const box = el('div'),
      chips = el('div', 'chips four');
    [
      ['utf-8', 'UTF-8'],
      ['gbk', 'GBK'],
      ['big5', 'Big5'],
      ['shift_jis', 'SJIS'],
    ].forEach(([k, t]) => {
      const b = el('button', 'chip', t);
      b.type = 'button';
      b.id = 'enc-' + k;
      b.setAttribute('aria-pressed', String(k === encoding));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        setEncoding(k);
      });
      chips.appendChild(b);
    });
    box.append(el('span', 'side-label dyn', T('文件名编码')), chips);
    body.appendChild(box);
  }
  if (!rawEntries.length) return;
  const check = el('button', 'btn soft ar-open dyn', checkWord());
  check.type = 'button';
  check.id = 'checkBtn';
  check.disabled = checking;
  check.addEventListener('mousedown', (e) => e.preventDefault());
  check.addEventListener('click', async () => {
    if (checking) return;
    sound();
    checking = true;
    check.disabled = true;
    const r = await own.checkArchive(password);
    checking = false;
    const bad = r.bad.length;
    checkBad = bad;
    const b = document.getElementById('checkBtn');
    if (!b) return;
    b.disabled = false;
    b.textContent = checkWord();
    if (!reduce) b.animate([{ opacity: 0.4 }, { opacity: 1 }], MOVE);
  });
  body.appendChild(check);
}
[previewBtn, infoBtn].forEach((b) =>
  b.addEventListener('click', () => {
    closeFind();
    closeExtract(true);
    renderPanel();
  })
);
function setEncoding(k) {
  if (k === encoding) return;
  encoding = k;
  const was = cwd;
  nodes = buildNodes(rawEntries);
  index();
  history = [snap()];
  hAt = 0;
  savedSnap = history[0];
  cwd = nodes.some((n) => n.path === was) ? was : '';
  picked = new Set();
  drawCrumbs();
  drawRows();
  renderPanel();
  shell.changed();
  if (!reduce) rowsEl.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}

// ---------- find: laid out as DotAny Text's, across every folder ----------
const findBox = el('div', 'cfind');
findBox.setAttribute('role', 'search');
const findWrap = el('span', 'fi'),
  findIn = document.createElement('input');
findIn.type = 'text';
findIn.id = 'findText';
findIn.spellcheck = false;
findIn.autocomplete = 'off';
findWrap.appendChild(findIn);
const findCount = el('span', 'f-count dyn'),
  fClose = el('button', 'f-b');
fClose.type = 'button';
fClose.id = 'findClose';
fClose.innerHTML = icon('close');
fClose.setAttribute('aria-label', '关闭');
fClose.addEventListener('mousedown', (e) => e.preventDefault());
fClose.addEventListener('click', () => {
  sound();
  closeFind();
});
const fRow = el('div', 'f-row');
fRow.append(findWrap, findCount, fClose);
findBox.appendChild(fRow);
shell.place(findBox);
inkfall(findIn);
shell.translates(findBox);
function openFind() {
  shell.closeAll();
  closeExtract(true);
  renderPanel();
  findBox.classList.add('on');
  findBtn.setAttribute('aria-pressed', 'true');
  setTimeout(() => {
    findIn.focus();
    findIn.select();
  }, 30);
}
function closeFind() {
  if (!findBox.classList.contains('on')) return false;
  findBox.classList.remove('on');
  findBtn.setAttribute('aria-pressed', 'false');
  findIn.value = '';
  findCount.textContent = '';
  findBox.classList.remove('none');
  findIn.blur();
  picked = new Set();
  drawRows();
  return true;
}
findIn.addEventListener('input', () => {
  picked = new Set();
  drawRows();
  const q = findIn.value.trim();
  findCount.textContent = !q ? '' : shown.length ? String(shown.length) : T('无匹配结果');
  findBox.classList.toggle('none', !!q && !shown.length);
});
findIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') closeFind();
});
findBtn.addEventListener('click', () => {
  sound();
  if (!closeFind()) openFind();
});

// ---------- the save card's own rows: how hard to compress, and a password ----------
let levelKey = 'std',
  passValue = '',
  wantPass = false;
const LEVELS = { fast: '快速', std: '均衡', tight: '极致体积' };
function drawSaveRows(box, fmt, estimate) {
  const dlg = box.closest('.dlg');
  if (!dlg.classList.contains('on')) {
    // a fresh card: the default level, no password
    levelKey = 'std';
    passValue = '';
    wantPass = false;
  }
  {
    const b0 = el('div'),
      chips = el('div', 'chips three');
    Object.entries(LEVELS).forEach(([k, t]) => {
      const b = el('button', 'chip dyn', T(t));
      b.type = 'button';
      b.id = 'chip-' + k;
      b.setAttribute('aria-pressed', String(k === levelKey));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        levelKey = k;
        [...chips.children].forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
        estimate();
      });
      chips.appendChild(b);
    });
    b0.append(el('span', 'side-label dyn', T('压缩级别')), chips);
    box.appendChild(b0);
  }
  if (fmt === 'tgz' || fmt === 'tar') return;
  const r = el('div', 'row compact ar-pass'),
    l = el('div', 'ar-pass-l'),
    word = el('b', 'dyn', T('设置密码')),
    i = document.createElement('input'),
    eye = el('button', 'f-b ar-eye'),
    tog = el('button', 'ar-tog');
  r.setAttribute('aria-checked', String(wantPass));
  i.type = 'password';
  i.id = 'passInput';
  i.autocomplete = 'new-password';
  i.spellcheck = false;
  i.value = passValue;
  i.setAttribute('aria-label', T('密码'));
  i.placeholder = T('输入密码');
  i.tabIndex = wantPass ? 0 : -1;
  i.addEventListener('input', () => {
    passValue = i.value;
  });
  i.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      document.getElementById('saveAsConfirm').click();
    }
  });
  eye.type = 'button';
  eye.id = 'passEye';
  eye.innerHTML = icon('eye');
  eye.setAttribute('aria-label', T('显示密码'));
  eye.tabIndex = wantPass ? 0 : -1;
  eye.addEventListener('mousedown', (e) => e.preventDefault());
  eye.addEventListener('click', () => {
    sound();
    const show = i.type === 'password';
    i.type = show ? 'text' : 'password';
    eye.innerHTML = icon(show ? 'eyeOff' : 'eye');
  });
  tog.type = 'button';
  tog.id = 'passToggle';
  tog.setAttribute('role', 'switch');
  tog.setAttribute('aria-checked', String(wantPass));
  tog.setAttribute('aria-label', T('设置密码'));
  tog.innerHTML = '<i class="tog"></i>';
  const flip = () => {
    sound();
    wantPass = !wantPass;
    r.setAttribute('aria-checked', String(wantPass));
    tog.setAttribute('aria-checked', String(wantPass));
    i.tabIndex = eye.tabIndex = wantPass ? 0 : -1;
    if (wantPass) setTimeout(() => i.focus(), 30);
    else {
      i.blur();
      i.type = 'password';
      eye.innerHTML = icon('eye');
    }
  };
  tog.addEventListener('mousedown', (e) => e.preventDefault());
  tog.addEventListener('click', flip);
  word.addEventListener('mousedown', (e) => e.preventDefault());
  word.addEventListener('click', flip);
  l.append(word, i, eye);
  r.append(l, tog);
  box.appendChild(r);
}
// the list as the Rust side wants it
const plainNodes = (list) =>
  list.map((n) => ({
    path: n.path.replace(/\/$/, ''),
    dir: n.dir,
    i: n.src ? n.src.i : null,
    add: n.add || null,
    u: n.u ?? null,
    t: n.time || null,
  }));
async function saveArchive(path, fmt) {
  if (nodes.some((n) => n.lock && n.src) && password == null) {
    if (kindExt === 'rar') return { status: 'failed', kind: 'damaged' };
    if (!(await needPassword(false))) return { status: 'failed', kind: 'password' };
  }
  const pass = wantPass && fmt !== 'tar' && fmt !== 'tgz' && passValue ? passValue : null;
  return own.saveArchive(path, fmt, levelKey, pass, password, plainNodes(nodes));
}
function afterSave(r) {
  const was = cwd;
  password = wantPass && passValue ? passValue : password;
  hold({ ...r, dir: r.path.replace(/[\\/][^\\/]*$/, '') }, true);
  if (nodes.some((n) => n.path === was)) {
    cwd = was;
    drawCrumbs();
    drawRows();
  }
}

// ---------- extracting: a card of the family's kind, rising from the dock ----------
const xDlg = el('div', 'dlg ar-x');
xDlg.setAttribute('role', 'dialog');
xDlg.setAttribute('aria-label', '解压全部');
xDlg.id = 'extractCard';
const xInput = document.createElement('input');
xInput.type = 'text';
xInput.id = 'extractName';
xInput.spellcheck = false;
xInput.autocomplete = 'off';
xInput.setAttribute('aria-label', '文件名');
const xFieldIn = el('span', 'field-in');
xFieldIn.appendChild(xInput);
const xField = el('label', 'field');
xField.append(xFieldIn);
const xRows = el('div', 'sv-rows'),
  xEst = el('p', 'warn-t dyn'),
  xWarn = el('div', 'warn');
xWarn.appendChild(xEst);
xEst.id = 'extractEstimate';
const xCancel = el('button', 'btn', '取消'),
  xGo = el('button', 'btn primary dyn', T('解压全部'));
xCancel.type = xGo.type = 'button';
xCancel.id = 'extractCancel';
xGo.id = 'extractConfirm';
const xFoot = el('div', 'dlg-foot');
xFoot.append(xWarn, xCancel, xGo);
xDlg.append(xField, xRows, xFoot);
document.body.appendChild(xDlg);
inkfall(xInput);
shell.translates(xDlg);
let xOpen = false,
  scopeAll = true,
  makeFolder = true,
  location_ = '';
const measure = document.createElement('canvas').getContext('2d');
function fitX() {
  const cs = getComputedStyle(xInput);
  measure.font = cs.fontWeight + ' ' + cs.fontSize + ' ' + cs.fontFamily;
  xFieldIn.style.width = Math.ceil(measure.measureText(xInput.value).width) + 3 + 'px';
}
function xProblem() {
  if (!makeFolder) return '';
  const v = xInput.value.trim();
  if (!v) return '文件名不能为空。';
  // the Mac's names only can't have / and : (Finder shows a : as /)
  if (os({ windows: /[\\/:*?"<>|]/, mac: /[/:]/ }).test(v) || v === '.' || v === '..')
    return os({ windows: '文件名不能包含以下字符：\\ / : * ? " < > |', mac: '文件名不能包含「/」或「:」。' });
  return '';
}
let xEstAt = 0;
function xEstimate() {
  const problem = xOpen ? xProblem() : '';
  // the card's button says what it will do: everything, or only what's picked
  xGo.textContent = T(scopeAll || !picked.size ? '解压全部' : '解压所选');
  xWarn.classList.toggle('bad', !!problem);
  xField.classList.toggle('bad', !!problem);
  const text = problem
    ? T(problem)
    : T('预计 {size}', { size: money((scopeAll ? under('') : scopeFiles()).reduce((a, n) => a + n.size, 0)) });
  if (xEst.textContent === text) return;
  xEst.textContent = text;
  const now = performance.now();
  if (now - xEstAt > 200) {
    xEst.classList.remove('swap');
    void xEst.offsetWidth;
    xEst.classList.add('swap');
  }
  xEstAt = now;
}
function segRow(items, cur, pick) {
  const seg = el('div', 'seg'),
    pl = el('i', 'pl');
  seg.appendChild(pl);
  const btns = items.map(([k, t]) => {
    const b = el('button', 'sg dyn', t);
    b.type = 'button';
    b.id = 'scope-' + k;
    b.setAttribute('aria-pressed', String(k === cur));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    seg.appendChild(b);
    return b;
  });
  const place = () => {
    const on = btns.find((b) => b.getAttribute('aria-pressed') === 'true');
    pl.style.transform = `translateX(${on.offsetLeft}px)`;
    pl.style.width = on.offsetWidth + 'px';
  };
  btns.forEach((b, i) =>
    b.addEventListener('click', () => {
      sound();
      btns.forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      place();
      pick(items[i][0]);
    })
  );
  // the capsule starts where it belongs when the card opens; it only slides when you choose
  requestAnimationFrame(() => {
    pl.style.transition = 'none';
    place();
    void pl.offsetWidth;
    pl.style.transition = '';
  });
  return seg;
}
const pathEl = el('span', 'ar-path');
function drawX() {
  xRows.textContent = '';
  const w = el('div', 'ar-where'),
    change = el('button', 'btn soft dyn', T('更改'));
  change.type = 'button';
  change.id = 'changeWhere';
  change.addEventListener('mousedown', (e) => e.preventDefault());
  change.addEventListener('click', async () => {
    sound();
    const p = await own.pickFolder(location_);
    if (p) {
      location_ = p;
      pathEl.textContent = p;
      pathEl.title = p;
    }
  });
  pathEl.textContent = location_;
  pathEl.title = location_;
  pathEl.id = 'extractWhere';
  w.append(el('span', 'ar-where-k dyn', T('目标路径')), pathEl, change);
  xRows.appendChild(w);
  const f = el('button', 'row compact');
  f.type = 'button';
  f.id = 'makeFolder';
  f.setAttribute('role', 'switch');
  f.setAttribute('aria-checked', String(makeFolder));
  f.innerHTML = `<span class="row-t"><b></b></span><i class="tog"></i>`;
  f.querySelector('b').textContent = T('自动创建同名文件夹');
  f.addEventListener('mousedown', (e) => e.preventDefault());
  f.addEventListener('click', () => {
    sound();
    morph(
      xDlg,
      () => {
        makeFolder = !makeFolder;
        f.setAttribute('aria-checked', String(makeFolder));
        xField.hidden = !makeFolder;
      },
      () => (makeFolder ? [xField] : [])
    );
    if (makeFolder) {
      fitX();
      setTimeout(() => {
        xInput.focus();
        xInput.select();
      }, 30);
    }
    xEstimate();
  });
  xRows.appendChild(f);
  if (picked.size)
    xRows.appendChild(
      segRow(
        [
          ['all', T('全部文件')],
          ['some', N.chosen(picked.size)],
        ],
        scopeAll ? 'all' : 'some',
        (k) => {
          scopeAll = k === 'all';
          xEstimate();
        }
      )
    );
}
// a sensible folder: when everything already sits in one folder, it isn\'t wrapped in another
const baseName = () =>
  fileName.replace(
    /\.(tar\.(gz|bz2|xz|zst)|tgz|tbz2?|txz|tzst|zip|7z|tar|gz|bz2|xz|zst|rar|lzh|lha|cpio|ar|deb|rpm)$/i,
    ''
  );
// one file, compressed (.gz, .xz ...): it comes out as itself, not in a folder of its own name, as gunzip does
const single = () => /^(gz|bz2|xz|zst)$/.test(kindExt);
function oneTop() {
  const tops = new Set(nodes.map((n) => n.path.split('/')[0]));
  return tops.size === 1 && nodes.some((n) => n.dir && parentOf(n.path) === '') ? [...tops][0] : null;
}
async function openExtract() {
  if (!opened || root.classList.contains('extracting')) return;
  if (xOpen) {
    closeExtract();
    return;
  }
  shell.closeAll();
  closeFind();
  sound();
  scopeAll = !picked.size;
  makeFolder = !oneTop() && !single();
  xField.hidden = !makeFolder;
  if (!location_ || !docDir) location_ = docDir || (await be.defaultFolder());
  xInput.value = baseName() || T('未命名');
  drawX();
  xOpen = true;
  const r = extractBtn.getBoundingClientRect(),
    box = pill.getBoundingClientRect();
  xDlg.style.top = 'auto';
  xDlg.style.bottom = innerHeight - box.top + 8 + 'px';
  xDlg.style.transformOrigin = '100% 100%';
  xDlg.style.left = clamp(r.right - xDlg.offsetWidth, 8, innerWidth - xDlg.offsetWidth - 8) + 'px';
  fitX();
  xEstimate();
  xDlg.classList.add('on');
  if (makeFolder)
    setTimeout(() => {
      xInput.focus();
      xInput.select();
    }, 30);
}
function closeExtract(quiet) {
  if (!xOpen) return false;
  if (quiet !== true) sound();
  xOpen = false;
  xDlg.classList.remove('on');
  xInput.blur();
  return true;
}
function confirmExtract() {
  if (xProblem()) {
    sound();
    xEstimate();
    xInput.focus();
    return;
  }
  sound();
  const all = scopeAll || !picked.size;
  const base = all ? '' : cwd;
  // the chosen items, and everything inside the chosen folders, named from where they sit
  const list = all
    ? nodes.slice()
    : nodes.filter((n) => [...picked].some((p) => n.path === p || (p.endsWith('/') && n.path.startsWith(p))));
  const items = list.map((n) => ({ ...n, path: n.path.slice(base.length) }));
  closeExtract(true);
  runExtract(makeFolder ? xInput.value.trim() : null, items);
}
xInput.addEventListener('input', () => {
  fitX();
  xEstimate();
});
xInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    confirmExtract();
  }
});
xDlg.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') closeExtract();
});
xCancel.addEventListener('click', () => closeExtract());
xGo.addEventListener('click', confirmExtract);
document.addEventListener('pointerdown', (e) => {
  if (xOpen && !xDlg.contains(e.target) && !extractBtn.contains(e.target) && !pwWrap.contains(e.target)) closeExtract();
});

// ---------- the progress line: the dock rises with it; orange marks only where it is now ----------
function morphDock(change, timing = GROW) {
  if (reduce) {
    change();
    return;
  }
  pill.getAnimations().forEach((a) => a.cancel());
  const w0 = pill.offsetWidth,
    h0 = pill.offsetHeight;
  change();
  const w1 = pill.offsetWidth,
    h1 = pill.offsetHeight;
  pill.style.overflow = 'hidden';
  const a = pill.animate(
    [
      { width: w0 + 'px', height: h0 + 'px' },
      { width: w1 + 'px', height: h1 + 'px' },
    ],
    timing
  );
  a.onfinish = a.oncancel = () => {
    pill.style.overflow = '';
  };
  const fresh = root.classList.contains('extracting') ? [prog] : [pillRow];
  fresh.forEach((e) => e.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 120, fill: 'backwards' }));
}
const progBtn = (id, text, cls, run) => {
  const b = el('button', 'btn ' + cls + ' dyn', T(text));
  b.type = 'button';
  b.id = id;
  b.dataset.k = text;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    run();
  });
  return b;
};
let running = false,
  lastTarget = '';
function leaveProgress() {
  morphDock(() => root.classList.remove('extracting', 'extracted'), MOVE);
  syncAll();
}
let shownP = 0,
  targetP = 0,
  progRaf = 0,
  progLast = 0,
  progWord = '正在解压',
  progFile = '';
function paintP() {
  const w = progLine.clientWidth;
  progFill.style.transform = `scaleX(${shownP})`;
  progNow.style.transform = `translateX(${shownP * w}px)`;
  progPct.textContent = Math.round(shownP * 100) + '%';
}
function tickP(t) {
  const dt = Math.min(64, t - (progLast || t));
  progLast = t;
  shownP += (targetP - shownP) * (1 - Math.exp(-dt / 70));
  paintP();
  if (root.classList.contains('extracting') && !root.classList.contains('extracted'))
    progRaf = requestAnimationFrame(tickP);
}
const paintName = () => {
  progName.textContent = T(progWord) + (progFile ? ' · ' + progFile : '');
};
be.on('progress', (p) => {
  if (!running) return;
  targetP = p.total ? Math.min(1, p.done / p.total) : 0;
  if (p.name && p.name !== progFile) {
    progFile = p.name;
    paintName();
  }
});
async function runExtract(folder, items) {
  if (items.some((n) => n.lock && n.src) && password == null) {
    if (kindExt === 'rar') {
      sound();
      say('暂不支持打开加密的 RAR 压缩包。');
      return;
    }
    if (!(await needPassword(true))) items = items.filter((n) => !(n.lock && n.src));
    if (!items.some((n) => !n.dir)) return;
  }
  const skippedLocked = password == null ? items.filter((n) => n.lock && n.src).length : 0;
  running = true;
  shownP = 0;
  targetP = 0;
  progLast = 0;
  progWord = '正在解压';
  progFile = '';
  paintP();
  paintName();
  progBtns.replaceChildren(progBtn('progCancel', '取消', 'soft', () => own.cancelJob()));
  morphDock(() => {
    root.classList.remove('extracted');
    root.classList.add('extracting');
  });
  extractBtn.disabled = true;
  cancelAnimationFrame(progRaf);
  progRaf = requestAnimationFrame(tickP);
  const r = await own.extractTo(plainNodes(items), location_, folder, password);
  running = false;
  if (r.status === 'cancelled') {
    progWord = '操作已取消。';
    progFile = '';
    leaveProgress();
    say('操作已取消。');
    return;
  }
  if (r.status === 'failed') {
    leaveProgress();
    sound();
    say(
      r.kind === 'diskfull'
        ? '目标磁盘空间不足，无法完成解压。'
        : r.kind === 'denied' || r.kind === 'readonly'
          ? '目标路径缺少写入权限，无法完成解压。'
          : '解压未能完成。'
    );
    return;
  }
  targetP = 1;
  await new Promise((ok) => setTimeout(ok, reduce ? 0 : 260));
  cancelAnimationFrame(progRaf);
  shownP = 1;
  paintP();
  root.classList.add('extracted');
  lastTarget = r.target;
  const bad = r.bad.length,
    skipped = r.skipped + r.locked + skippedLocked;
  progResult = { bad, skipped, target: r.target };
  paintResult();
  progBtns.replaceChildren(
    progBtn('progFolder', os({ windows: '在资源管理器中显示', mac: '在访达中显示' }), 'soft', () =>
      own.reveal(lastTarget)
    ),
    progBtn('progDone', '完成', 'primary', () => leaveProgress())
  );
  if (!reduce) progBtns.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}
let progResult = null;
function paintResult() {
  if (!progResult || !root.classList.contains('extracted')) return;
  const { bad, skipped, target } = progResult;
  progName.textContent = bad ? N.damaged(bad) : T('已解压至') + ' ' + target + (skipped ? N.skipped(skipped) : '');
  progName.title = target;
  progPct.textContent = '';
}

// ---------- opening ----------
function buildNodes(entries) {
  const map = new Map();
  for (const e of entries) {
    const raw = decodeName(e, encoding).replace(/\\/g, '/');
    const isDir = e.d || raw.endsWith('/');
    // "./a.txt" and "a//b" are plain names; a name that climbs out of its folder is how archives overwrite your files:
    // it's never honoured
    const parts = raw.split('/').filter((s) => s && s !== '.');
    if (!parts.length || parts.includes('..')) continue;
    const path = parts.join('/') + (isDir ? '/' : '');
    const time = e.t || (e.u != null ? stamp(new Date(e.u * 1000)) : '');
    const u = e.u != null ? e.u : null;
    if (isDir) map.set(path, { path, dir: true, size: 0, time, u, src: e });
    else map.set(path, { path, dir: false, size: e.s, time, u, lock: e.l, src: e });
    const bits = path.replace(/\/$/, '').split('/');
    bits.pop();
    let at = '';
    for (const b of bits) {
      at += b + '/';
      if (!map.has(at)) map.set(at, { path: at, dir: true, size: 0, time, u });
    }
  }
  // a folder named in the archive as a folder doesn't carry an index into the data: only files are read
  for (const n of map.values()) if (n.dir) delete n.src;
  return [...map.values()];
}
async function openArchive(path) {
  if (path === NEW)
    return {
      status: 'ok',
      path: '',
      dir: await be.defaultFolder(),
      name: T('未命名') + '.zip',
      kind: 'zip',
      entries: [],
      size: 0,
      items: [],
    };
  let r = await own.openFile(path);
  if (r.status === 'locked') {
    const name = path.replace(/^.*[\\/]/, '');
    let good = null;
    try {
      await askPassword(false, {
        name,
        test: async (v) => {
          const t = await own.openFile(path, v);
          if (t.status === 'ok') {
            good = t;
            return 'ok';
          }
          return t.status === 'wrong' ? 'wrong' : 'ok';
        },
      });
    } catch {
      return { status: 'abandoned' };
    }
    if (!good) return { status: 'damaged' };
    r = good;
    r.headerPassword = true;
  }
  if (r.status === 'fresh')
    return { status: 'ok', path: '', dir: r.dir, name: r.name, kind: 'zip', entries: [], size: 0, items: r.items };
  return r;
}
function hold(r, keepPassword) {
  shell.closeAll();
  closeFind();
  closeExtract(true);
  if (root.classList.contains('extracting')) {
    if (running) own.cancelJob();
    root.classList.remove('extracting', 'extracted');
  }
  if (!r.path) own.forgetSource();
  rawEntries = (r.entries || []).map((e) => (e.r ? { ...e, raw: fromB64(e.r) } : e));
  if (!keepPassword) password = r.headerPassword ? null : null;
  encoding = guessEncoding(rawEntries);
  hasOdd = rawEntries.some((e) => e.raw);
  nodes = buildNodes(rawEntries);
  index();
  fileName = r.name;
  fileSize = r.size || 0;
  kindExt = r.kind || 'zip';
  docDir = r.dir || '';
  opened = true;
  cwd = '';
  picked = new Set();
  anchor = null;
  checkBad = null;
  location_ = docDir;
  history = [snap()];
  hAt = 0;
  savedSnap = history[0];
  stage.scrollTop = 0;
  drawCrumbs();
  drawHead();
  drawRows();
  renderPanel();
  if (!reduce) listEl.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  if (r.items && r.items.length) addItems(r.items);
}
function clearAll() {
  closeFind();
  closeExtract(true);
  if (running) own.cancelJob();
  running = false;
  root.classList.remove('extracting', 'extracted');
  opened = false;
  nodes = [];
  rawEntries = [];
  index();
  cwd = '';
  password = null;
  picked = new Set();
  fileName = '';
  kindExt = '';
  crumbs.textContent = '';
  history = [];
  hAt = 0;
  savedSnap = null;
}

// ---------- keys, after the family's own ----------
function onKey(e) {
  const mod = modKey(e),
    k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName);
  if (!opened || pwWrap.classList.contains('on') || document.querySelector('.ask-wrap.on')) return;
  if (mod && k.toLowerCase() === 'f') {
    e.preventDefault();
    openFind();
    return;
  }
  if (k === 'Escape') {
    if (closeFind() || closeExtract()) return;
    if (picked.size) {
      picked = new Set();
      syncAll();
      renderPanel();
    }
    return;
  }
  if (typing) return;
  if (mod && k.toLowerCase() === 'z') {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
    return;
  }
  if (mod && k.toLowerCase() === 'y') {
    e.preventDefault();
    redo();
    return;
  }
  if (mod && k.toLowerCase() === 'a') {
    e.preventDefault();
    picked = new Set(shown.map((n) => n.path));
    syncAll();
    renderPanel();
    return;
  }
  if (k === ' ') {
    e.preventDefault();
    previewBtn.click();
    return;
  }
  if (k === 'Delete') {
    removePicked();
    return;
  }
  if (k === 'F2') {
    e.preventDefault();
    startRename();
    return;
  }
  if (k === 'Backspace') {
    e.preventDefault();
    goUp();
    return;
  }
  if (k === 'Enter') {
    if (picked.size === 1) openNode(nodes.find((n) => picked.has(n.path)));
    return;
  }
  if (k === 'ArrowDown' || k === 'ArrowUp') {
    e.preventDefault();
    const list = shown.map((n) => n.path);
    if (!list.length) return;
    const at = anchor ? list.indexOf(anchor) : -1,
      to = clamp(at + (k === 'ArrowDown' ? 1 : -1), 0, list.length - 1);
    anchor = list[to];
    picked = new Set([anchor]);
    if (shown.length > MANY) {
      const y = to * ROW + rowsEl.offsetTop;
      if (y < stage.scrollTop + 93 || y + ROW > stage.scrollTop + stage.clientHeight - 96)
        stage.scrollTop = y - stage.clientHeight / 2;
      paintRows(false);
    }
    syncAll();
    renderPanel();
    const r = [...rowsEl.children].find((x) => x.dataset.path === anchor);
    if (r) r.scrollIntoView({ block: 'nearest' });
  }
}

// ---------- the family's parts, switched on for this tool ----------
// the right-click menu: a switch on the family's Interface page, for this user only, off until turned on
const menuLabels = () => [T('使用 DotAny Archive 打开'), T('使用 DotAny Archive 压缩')];
const menuRow = shell.settingRow('interface', {
  id: 'shellMenuRow',
  // the Finder's Quick Actions on the Mac
  title: os({ windows: '集成至系统右键菜单', mac: '集成至访达快速操作' }),
  note: os({
    windows:
      '在文件资源管理器的右键菜单中添加「使用 DotAny Archive 打开」与「使用 DotAny Archive 压缩」（仅对当前账户生效）。',
    mac: '在访达的「快速操作」中添加「使用 DotAny Archive 打开」与「使用 DotAny Archive 压缩」。',
  }),
  get: () => own.shellMenu(),
  set: async (on) => {
    if (!(await own.setShellMenu(on, ...menuLabels())))
      say(os({ windows: '无法更新系统右键菜单设置。', mac: '无法更新快速操作设置。' }));
  },
});
document.getElementById('settings').addEventListener('click', () => {
  closeFind();
  closeExtract(true);
});
document.getElementById('saveAs').addEventListener('click', () => {
  closeFind();
  closeExtract(true);
});

// words this tool draws itself
shell.onLang(() => {
  pwInput.placeholder = T('输入密码');
  findIn.placeholder = T('查找');
  if (menuRow.getAttribute('aria-checked') === 'true') own.setShellMenu(true, ...menuLabels());
  if (opened && !shell.doc.path) fileName = shell.fileName;
  progBtns.querySelectorAll('[data-k]').forEach((b) => {
    b.textContent = T(b.dataset.k);
  });
  syncAll();
  if (opened) {
    drawHead();
    drawRows();
    renderPanel();
    paintResult();
    if (root.classList.contains('extracting') && !root.classList.contains('extracted')) paintName();
  }
  if (xOpen) {
    drawX();
    xEstimate();
  }
});

// with DOTANY_TEST set, shell.test exposes the open archive and its actions, so that an automated test can read and drive it
Object.defineProperties(
  shell.test,
  Object.getOwnPropertyDescriptors({
    get nodes() {
      return nodes;
    },
    get cwd() {
      return cwd;
    },
    get picked() {
      return [...picked];
    },
    get encoding() {
      return encoding;
    },
    get shown() {
      return shown.map((n) => n.path);
    },
    get password() {
      return password;
    },
    get kind() {
      return kindExt;
    },
    get hasOdd() {
      return hasOdd;
    },
    setWhere(p) {
      location_ = p;
      pathEl.textContent = p;
      pathEl.title = p;
    },
    go,
    pick(paths) {
      picked = new Set(paths);
      syncAll();
      renderPanel();
    },
    addItems,
    plainNodes: () => plainNodes(nodes),
    own,
  })
);
own.siblings().then((list) => {
  installed = list;
});
// the menu's words follow the language even when it changed while the program was closed
own.shellMenu().then((on) => {
  if (on) own.setShellMenu(true, ...menuLabels());
});
shell.begin();
