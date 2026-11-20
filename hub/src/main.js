// DotAny, the program: the family's home. A column of places on the left (Home, Convert, Tools, Default Apps; Settings,
// Contact Us, About at its foot) and one page on the right. No top bar: the window's own buttons sit at the top right,
// and the empty parts of the window move it.
// Started by the right-click menu (dotany.exe --convert …), the same page shows only the converter, in a small window.
import * as be from '@dotany/web/backend.js';
import { fontsChosen } from '@dotany/web/fonts.js';
import { el, root, reduce, store, sound, icon, LUCIDE, tool, EASE } from '@dotany/web/shared.js';
import {
  T,
  lang,
  setLang,
  addTables,
  pickLang,
  LANGS,
  langInfo,
  langReady,
  loadLang,
  size,
  date,
  testLanguage,
} from '@dotany/web/i18n.js';
import { HARMONY_LICENCE } from '@dotany/web/licences.js';
import { inkfall } from '@dotany/web/inkfall-input.js';
import { mountWindowControls, syncFrame, setWindowTitle } from '@dotany/web/window-frame.js';
import { languageRow, fitChips, barBtn } from '@dotany/web/parts.js';
import { LUCIDE as ICONS } from './icons.js';
import * as hub from './backend.js';
import { modKey, os, platform } from '@dotany/web/keys.js';

Object.assign(LUCIDE, ICONS);
tool.name = 'DotAny';
const boot = window.__DOTANY__ || {};
const t0 = performance.now();
const solo = boot.view === 'convert';
// DotAny's own words in other languages: src/lang/<code>.json, added to the family's (shared/web/lang)
import en from './lang/en.json';
// English is always here; every other language is loaded when chosen (shared/web/i18n.js LANGS lists them)
addTables({
  en,
  'zh-Hant': () => import('./lang/zh-Hant.json'),
  ja: () => import('./lang/ja.json'),
  ko: () => import('./lang/ko.json'),
  'es-419': () => import('./lang/es-419.json'),
  'pt-BR': () => import('./lang/pt-BR.json'),
  fr: () => import('./lang/fr.json'),
  de: () => import('./lang/de.json'),
});
const MOVE = { duration: 240, easing: EASE };
const money = (b) => size(b, { zero: true, gb: true });
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- the family as it is on this computer ----------
const META = {
  text: ['doc', '文档与代码'],
  image: ['image', '图像'],
  video: ['film', '视频'],
  audio: ['music', '音频'],
  pdf: ['pdf', '阅览与编辑'],
  archive: ['archive', '压缩包'],
  sheets: ['sheet', '表格'],
  slides: ['slide', '演示文稿'],
  recorder: ['mic', '录音'],
  books: ['book', '电子书'],
  model: ['box', '三维模型'],
};
let TOOLS = [];
const toolOf = (k) => TOOLS.find((t) => t.key === k);
let EXT_TOOL = {};
async function loadTools() {
  TOOLS = (await hub.tools()).map((t) => ({
    key: t.key,
    name: t.name,
    ic: META[t.key][0],
    note: META[t.key][1],
    ver: t.version || '',
    state: t.installed ? 'in' : 'off',
    exts: t.exts,
    rare: t.rare || [],
  }));
  // the rare formats go to their tool too, after the common ones; Default Apps shows chips for t.exts only
  EXT_TOOL = {};
  TOOLS.forEach((t) =>
    [...t.exts, ...t.rare].forEach((x) => {
      if (!EXT_TOOL[x]) EXT_TOOL[x] = t.key;
    })
  );
}
const extOf = (n) => (n.includes('.') ? n.split('.').pop().toLowerCase() : '');
const iconOf = (n, key) => {
  const t = toolOf(key || EXT_TOOL[extOf(n)]);
  return t ? t.ic : 'doc';
};
// the shared list: every tool writes to it, DotAny reads it
let recents = [];
const loadRecents = async () => {
  recents = await hub.recents();
};

// ---------- the window: a column of places, and a page ----------
const app = document.getElementById('app');
const rail = el('nav', 'rail'),
  navBox = el('div', 'nav'),
  navPl = el('i', 'nav-pl'),
  mainEl = el('main', 'main'),
  pageIn = el('div', 'pg-in');
mainEl.id = 'main';
mainEl.appendChild(pageIn);
const navItem = (id, n, label) => {
  const b = el('button', 'nav-i');
  b.type = 'button';
  b.id = id;
  b.setAttribute('aria-label', label);
  b.innerHTML = icon(n);
  b.append(el('span', 'nav-t', label));
  b.addEventListener('mousedown', (e) => e.preventDefault());
  return b;
};
const PLACES = [
  ['home', 'home', '主页'],
  ['convert', 'convert', '转换'],
  ['tools', 'tools', '工具'],
  ['defaults', 'check', '默认应用'],
  ['settings', 'settings', '设置'],
  ['contact', 'mail', '联系我们'],
  ['about', 'info', '关于'],
];
const placeBtns = PLACES.map(([k, ic, t]) => {
  const b = navItem('nav-' + k, ic, t);
  b.addEventListener('click', () => {
    sound();
    go(k);
  });
  return b;
});
const railGap = el('i', 'rail-gap'),
  railName = el('p', 'rail-name', 'DotAny');
navBox.append(navPl, ...placeBtns.slice(0, 4), railGap, ...placeBtns.slice(4));
rail.append(railName, navBox);
const toast = el('div', 'toast');
toast.id = 'toast';
// the window's own buttons, the family's, where the tools have them; the window has no other bar
const winBox = el('div', 'meta win');
app.append(rail, mainEl, toast, winBox);
mountWindowControls(winBox, winBox, []);
// the converter's small window has nothing to fill a screen with
// (on the Mac the system draws the window's buttons, so there are none of the family's to take away)
if (solo) ['#winMax', '#winRestore'].forEach((s) => winBox.querySelector(s)?.remove());
// the empty parts of the window move it, and a right click there is the window's own menu
[rail, railName, navBox, railGap, mainEl, pageIn].forEach((e) => e.setAttribute('data-tauri-drag-region', ''));
addEventListener('contextmenu', (e) => {
  if (e.target.hasAttribute && e.target.hasAttribute('data-tauri-drag-region')) {
    e.preventDefault();
    be.windowMenu(Math.round(e.screenX), Math.round(e.screenY));
    return;
  }
  // WebView2's own menu has Back, Refresh and Print in it; only what can be typed in keeps one
  if (!(e.target.closest && e.target.closest('input, textarea, .lic-text'))) e.preventDefault();
});
// and its keys: a reload, find, print preview
addEventListener(
  'keydown',
  (e) => {
    const k = e.key.toLowerCase();
    if (
      k === 'f5'
      || (modKey(e) && !e.altKey && k.length === 1 && (platform === 'mac' ? 'rfgjudkep' : 'rfghjudkep').includes(k))
    )
      e.preventDefault();
  },
  true
);
be.on('close-requested', () => be.closeWindow());
function say(text) {
  toast.textContent = text;
  toast.classList.add('on');
  clearTimeout(say.t);
  say.t = setTimeout(() => toast.classList.remove('on'), 4200);
}

// whatever changes size eases to it, and what's new fades in; nothing jumps
function morph(box, change, fresh = () => [...box.children]) {
  if (reduce) {
    change();
    return;
  }
  box.getAnimations().forEach((a) => a.cancel());
  const from = box.offsetHeight;
  change();
  const to = box.offsetHeight;
  fresh().forEach((c) => c.animate([{ opacity: 0 }, { opacity: 1 }], MOVE));
  if (from === to) return;
  box.style.overflow = 'hidden';
  const a = box.animate([{ height: from + 'px' }, { height: to + 'px' }], MOVE);
  a.onfinish = a.oncancel = () => {
    box.style.overflow = '';
  };
}

let place = 'home';
function placePill() {
  const on = placeBtns[PLACES.findIndex((p) => p[0] === place)];
  placeBtns.forEach((b) => b.setAttribute('aria-current', String(b === on)));
  navPl.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
  navPl.style.width = on.offsetWidth + 'px';
  navPl.style.height = on.offsetHeight + 'px';
}
function paintPage() {
  pageIn.textContent = '';
  ({
    home: drawHome,
    convert: drawConvert,
    tools: drawTools,
    defaults: drawDefaults,
    settings: drawSettings,
    contact: drawContact,
    about: drawAbout,
  })[place]();
}
function go(k, keep) {
  if (k === place && !keep) return;
  place = k;
  placePill();
  mainEl.scrollTop = 0;
  paintPage();
  if (!reduce) pageIn.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}
const pageHead = (title, ...acts) => {
  const h = el('div', 'pg-head');
  h.setAttribute('data-tauri-drag-region', '');
  h.appendChild(el('h1', 'pg-title', T(title)));
  if (acts.length) {
    const a = el('div', 'pg-acts');
    a.append(...acts);
    h.appendChild(a);
  }
  return h;
};
const btn = (cls, text, id, run) => {
  const b = el('button', 'btn ' + cls, T(text));
  b.type = 'button';
  if (id) b.id = id;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', run);
  return b;
};

// ---------- home: drop, make, carry on ----------
const MAKES = [
  { key: 'doc', ic: 'doc', title: '文档', tool: 'text', ext: 'docx' },
  { key: 'sheet', ic: 'sheet', title: '表格', tool: 'sheets', ext: 'xlsx' },
  { key: 'slide', ic: 'slide', title: '演示文稿', tool: 'slides', ext: 'pptx' },
  { key: 'zip', ic: 'archive', title: '压缩包', tool: 'archive', ext: 'zip' },
  { key: 'rec', ic: 'mic', title: '录音', tool: 'recorder', ext: 'wav' },
];
// everything the family can start from nothing: the same panel as DotAny Text, with the other tools' files beside its own
const NEW_GROUPS = [
  {
    key: 'doc',
    label: '文档',
    tool: 'text',
    items: [
      ['Word', '.docx', '文档 document'],
      ['Markdown', '.md', '文档 document'],
      ['纯文本', '.txt', 'text plain 文本 便签'],
    ],
  },
  {
    key: 'sheet',
    label: '表格',
    tool: 'sheets',
    items: [
      ['Excel 工作簿', '.xlsx', '表格 excel sheet'],
      ['CSV', '.csv', '表格 逗号 sheet'],
      ['TSV', '.tsv', '表格 制表符 sheet'],
    ],
  },
  { key: 'slide', label: '演示文稿', tool: 'slides', items: [['PowerPoint', '.pptx', '幻灯片 演示 ppt slides']] },
  {
    key: 'zip',
    label: '压缩包',
    tool: 'archive',
    items: [
      ['ZIP', '.zip', '压缩 archive'],
      ['7z', '.7z', '压缩 archive'],
      ['TAR', '.tar', '压缩 archive'],
    ],
  },
  {
    key: 'rec',
    label: '录音',
    tool: 'recorder',
    items: [
      ['WAV', '.wav', '录音 声音 record'],
      ['FLAC', '.flac', '录音 声音 record'],
      ['MP3', '.mp3', '录音 声音 record'],
    ],
  },
  {
    key: 'code',
    label: '代码',
    tool: 'text',
    items: [
      ['Python', '.py', '代码 code'],
      ['JavaScript', '.js', '代码 code'],
      ['TypeScript', '.ts', '代码 code'],
      ['HTML', '.html', '代码 code 网页'],
      ['CSS', '.css', '代码 code 样式'],
    ],
  },
  {
    key: 'data',
    label: '数据与配置',
    tool: 'text',
    items: [
      ['JSON', '.json', '数据 配置 data config'],
      ['YAML', '.yaml', 'yml 数据 配置 data config'],
      ['TOML', '.toml', '数据 配置 data config'],
      ['XML', '.xml', '数据 配置 data config'],
      ['INI', '.ini', '数据 配置 data config'],
    ],
  },
  {
    key: 'sub',
    label: '字幕',
    tool: 'text',
    items: [
      ['SRT', '.srt', '字幕 subtitle'],
      ['VTT', '.vtt', '字幕 subtitle'],
    ],
  },
].map((g) => ({ ...g, items: g.items.map(([label, ext, also]) => ({ label, ext, also, tool: g.tool })) }));
const ALL_NEW = NEW_GROUPS.flatMap((g) => g.items);
const newWrap = el('div', 'new-wrap');
newWrap.setAttribute('role', 'dialog');
newWrap.setAttribute('aria-modal', 'true');
newWrap.id = 'newPanel';
const newBox = el('div', 'new'),
  newSearch = el('label', 'new-search'),
  newField = el('span', 'fi'),
  newInput = document.createElement('input');
newInput.type = 'text';
newInput.id = 'newSearch';
newInput.spellcheck = false;
newInput.autocomplete = 'off';
newField.appendChild(newInput);
newSearch.innerHTML = icon('search');
newSearch.appendChild(newField);
const newBody = el('div', 'new-body'),
  newNav = el('nav', 'set-nav new-nav'),
  newPill = el('i', 'set-pl'),
  newList = el('div', 'new-list');
newNav.appendChild(newPill);
let newPage = 'doc',
  newRows = [],
  newActive = -1;
const newTabs = NEW_GROUPS.map((g) => {
  const b = el('button', 'set-tab');
  b.type = 'button';
  b.dataset.k = g.key;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', () => {
    sound();
    const from = NEW_GROUPS.findIndex((x) => x.key === newPage),
      to = NEW_GROUPS.findIndex((x) => x.key === g.key);
    newPage = g.key;
    newInput.value = '';
    drawNew(Math.sign(to - from));
    newInput.focus();
  });
  newNav.appendChild(b);
  return b;
});
newBody.append(newNav, newList);
newBox.append(newSearch, newBody);
newWrap.appendChild(newBox);
document.body.appendChild(newWrap);
inkfall(newInput);
function placeNewPill() {
  const searching = !!newInput.value.trim();
  newTabs.forEach((b) => b.setAttribute('aria-current', String(!searching && b.dataset.k === newPage)));
  const on = newTabs.find((b) => b.dataset.k === newPage);
  newPill.style.opacity = searching ? 0 : 1;
  newPill.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
  newPill.style.width = on.offsetWidth + 'px';
  newPill.style.height = on.offsetHeight + 'px';
}
function markNew() {
  newRows.forEach((b, i) => b.classList.toggle('on', i === newActive));
  if (newRows[newActive]) newRows[newActive].scrollIntoView({ block: 'nearest' });
}
function drawNew(dir) {
  const q = newInput.value.trim().toLowerCase(),
    items = q
      ? ALL_NEW.filter((f) => (f.label + ' ' + f.ext + ' ' + f.also).toLowerCase().includes(q))
      : NEW_GROUPS.find((g) => g.key === newPage).items;
  newList.textContent = '';
  newRows = items.map((f) => {
    const b = el('button', 'new-row');
    b.type = 'button';
    b.append(el('span', '', T(f.label)), el('span', 'new-ext', f.ext));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      pickNew(f);
    });
    newList.appendChild(b);
    return b;
  });
  if (!items.length) newList.appendChild(el('p', 'new-none', T('未找到匹配的格式。')));
  newActive = q && items.length ? 0 : -1;
  markNew();
  placeNewPill();
  if (dir && !reduce)
    newList.animate(
      [
        { opacity: 0, transform: `translateY(${10 * dir}px)` },
        { opacity: 1, transform: 'none' },
      ],
      MOVE
    );
}
const pickNew = (f) => {
  closeNew();
  makeIn(f.tool, f.ext);
};
function closeNew() {
  if (!newWrap.classList.contains('on')) return false;
  newWrap.classList.remove('on');
  newInput.blur();
  return true;
}
function openNew(page) {
  newPage = page || 'doc';
  newTabs.forEach((b) => {
    b.textContent = T(NEW_GROUPS.find((g) => g.key === b.dataset.k).label);
  });
  newInput.placeholder = T('搜索格式');
  newInput.value = '';
  newWrap.classList.add('on');
  drawNew();
  requestAnimationFrame(placeNewPill);
  setTimeout(() => newInput.focus(), 30);
}
newInput.addEventListener('input', () => drawNew());
newInput.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') {
    sound();
    closeNew();
    return;
  }
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    if (newRows.length) {
      newActive = (newActive + (e.key === 'ArrowDown' ? 1 : -1) + newRows.length) % newRows.length;
      markNew();
    }
    return;
  }
  if (e.key === 'Enter' && newRows[newActive < 0 ? 0 : newActive]) {
    e.preventDefault();
    newRows[newActive < 0 ? 0 : newActive].click();
  }
});
newWrap.addEventListener('pointerdown', (e) => {
  if (e.target === newWrap) {
    sound();
    closeNew();
  }
});

// a tool's window opens on its own: DotAny only starts it. One that isn't here asks to be installed first
const couldNotStart = (t) => T('未能打开 DotAny {name}。', { name: t.name });
async function makeIn(toolKey, ext) {
  const t = toolOf(toolKey);
  if (!t) return;
  if (t.state === 'off') {
    askInstall(t);
    return;
  }
  const r = await hub.make(t.key, ext.replace(/^\./, ''));
  if (r === 'uninstalled') {
    await refreshTools();
    askInstall(t);
  } else if (r !== 'ok') say(couldNotStart(t));
}
async function openWith(path, toolKey) {
  const t = toolOf(toolKey) || toolOf(EXT_TOOL[extOf(path)]);
  if (!t) {
    say(T('暂不支持该文件格式。'));
    return;
  }
  if (t.state === 'off') {
    askInstall(t);
    return;
  }
  const r = await hub.open(path, t.key);
  if (r === 'missing') {
    say(T('未找到该文件，可能已被移动或删除。'));
    await reloadRecents();
  } else if (r === 'uninstalled') {
    await refreshTools();
    askInstall(t);
  } else if (r === 'refused') say(T('暂不支持该文件格式。'));
  else if (r !== 'ok') say(couldNotStart(t));
}
// when: Just now (刚刚), Today (今天) 14:20, Yesterday (昨天) 20:16, then the date (Oct 30; 10 月 30 日 in Chinese);
// nothing for what a tool remembered before the list was shared
function whenText(at) {
  if (!at) return '';
  const d = new Date(at),
    now = new Date(),
    day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`,
    days = Math.round((day(now) - day(d)) / 86400000);
  if (Date.now() - at < 60000 && days === 0) return T('刚刚');
  if (days === 0) return `${T('今天')} ${hm}`;
  if (days === 1) return `${T('昨天')} ${hm}`;
  const same = d.getFullYear() === now.getFullYear();
  return date(d, same ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
}
// whether the converter takes a file is the converter's answer (dotany-convert's targets), carried on each recent row
const asFile = (r) => ({ path: r.path, name: r.name, size: r.size });
function recentRow(r, picking) {
  const row = el('div', 'rec' + (r.gone ? ' gone' : ''));
  row.setAttribute('role', 'button');
  row.tabIndex = 0;
  row.dataset.name = r.name;
  row.dataset.path = r.path;
  const t = toolOf(r.tool || EXT_TOOL[extOf(r.name)]);
  row.innerHTML = icon(r.gone ? 'gone' : iconOf(r.name, r.tool));
  row.append(
    el('span', 'rec-n', r.name),
    el('span', 'rec-tool', t ? t.name : ''),
    el('span', 'rec-when', r.gone ? T('未找到文件') : whenText(r.at))
  );
  const label = picking ? '' : r.gone ? '移除' : r.convertible ? '转换' : '';
  if (label) {
    const a = el('button', 'rec-act', T(label));
    a.type = 'button';
    a.addEventListener('mousedown', (e) => e.preventDefault());
    a.addEventListener('click', (e) => {
      e.stopPropagation();
      sound();
      if (r.gone) removeRecent(r, row);
      else startConvert([asFile(r)]);
    });
    row.appendChild(a);
  } else row.querySelector('.rec-when').classList.add('keep');
  const open = () => {
    if (r.gone) return;
    sound();
    if (picking) startConvert([asFile(r)]);
    else openWith(r.path, r.tool);
  };
  row.addEventListener('click', open);
  row.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') open();
  });
  return row;
}
let recentsEl = null;
const homeList = () => recents.slice(0, 8);
/** The recent list's tool and date columns as wide as their longest words, never narrower than 72 and 96 px: every row the
 *  same, in any language (the layout adapts to the sentence). Measured again when rows, size or fonts change. */
const textW = (e) => {
  const r = document.createRange();
  r.selectNodeContents(e);
  return Math.ceil(r.getBoundingClientRect().width);
};
function fitRecCols(box) {
  if (!box.isConnected) return;
  const w = (sel, min) => Math.max(min, ...[...box.querySelectorAll(sel)].map(textW));
  box.style.setProperty('--rec-tool', w('.rec-tool', 72) + 'px');
  box.style.setProperty('--rec-when', w('.rec-when', 96) + 'px');
}
function watchRecCols(box) {
  const fit = () => requestAnimationFrame(() => fitRecCols(box));
  new MutationObserver(fit).observe(box, { childList: true, subtree: true, characterData: true });
  new ResizeObserver(fit).observe(box);
  if (document.fonts) document.fonts.addEventListener('loadingdone', fit);
}
function fillRecents() {
  recentsEl.textContent = '';
  if (!recents.length) {
    recentsEl.appendChild(el('p', 'rec-none', T('暂无最近打开的文件。')));
    return;
  }
  homeList().forEach((r) => recentsEl.appendChild(recentRow(r)));
}
function drawHome() {
  const makeBox = el('section'),
    tiles = el('div', 'tiles');
  makeBox.append(el('p', 'label', T('新建')), tiles);
  MAKES.forEach((m) => {
    // the Mac has no installer to bring one tool back: a tool taken out of DotAny's folder has no tile
    if (platform === 'mac' && toolOf(m.tool) && toolOf(m.tool).state === 'off') return;
    const b = el('button', 'tile');
    b.type = 'button';
    b.id = 'make-' + m.key;
    const t = toolOf(m.tool);
    b.innerHTML = icon(m.ic);
    b.append(el('b', '', T(m.title)), el('small', '', t.state === 'off' ? T('待安装') : `DotAny ${t.name}`));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      makeIn(m.tool, m.ext);
    });
    tiles.appendChild(b);
  });
  const more = el('button', 'tile');
  more.type = 'button';
  more.id = 'make-more';
  more.innerHTML = icon('plus');
  more.append(el('b', '', T('更多')), el('small', '', T('代码、字幕等')));
  more.addEventListener('mousedown', (e) => e.preventDefault());
  more.addEventListener('click', () => {
    sound();
    openNew('doc');
  });
  tiles.appendChild(more);
  const recentBox = el('section'),
    labelRow = el('div', 'label-row'),
    clear = el('button', 'text-btn', T('清空'));
  clear.type = 'button';
  clear.id = 'clearRecents';
  clear.addEventListener('click', () => {
    sound();
    askClear();
  });
  labelRow.append(el('p', 'label', T('最近打开')), clear);
  recentsEl = el('div', 'recents');
  recentsEl.id = 'recents';
  watchRecCols(recentsEl);
  recentBox.append(labelRow, recentsEl);
  fillRecents();
  pageIn.append(
    pageHead(
      '拖拽文件至此',
      btn('primary', '打开本地文件', 'homeOpen', () => {
        sound();
        pickOpen();
      })
    ),
    makeBox,
    recentBox
  );
}
function removeRecent(r, row) {
  recents = recents.filter((x) => x !== r);
  hub.forget(r.path);
  if (reduce) {
    fillRecents();
    return;
  }
  row.style.pointerEvents = 'none';
  const a = row.animate(
    [
      { height: row.offsetHeight + 'px', opacity: 1 },
      { height: 0, opacity: 0 },
    ],
    MOVE
  );
  a.onfinish = () => fillRecents();
}
// another tool opened or saved something, or DotAny wrote a converted file: only what changed moves
const listKey = (list) => list.map((r) => r.path + ':' + r.at + (r.gone ? '!' : '')).join('|');
async function reloadRecents() {
  const was = listKey(homeList()),
    wasConv = listKey(recFor());
  await loadRecents();
  if (place === 'home' && recentsEl && recentsEl.isConnected && listKey(homeList()) !== was)
    morph(recentsEl, fillRecents);
  if (place === 'convert' && convRec.isConnected && !busy && listKey(recFor()) !== wasConv) growConvRec();
  else if (place === 'convert' && !solo && !convRec.isConnected && recFor().length && !busy) go('convert', true);
}

// ---------- asking: the family's prompt ----------
const ask = el('div', 'ask-wrap');
ask.setAttribute('role', 'alertdialog');
ask.setAttribute('aria-modal', 'true');
const askBox = el('div', 'ask'),
  askTitle = el('p', 'ask-title'),
  askRow = el('div', 'ask-row'),
  aCancel = el('button', 'btn soft'),
  aYes = el('button', 'btn primary');
aCancel.type = aYes.type = 'button';
aCancel.id = 'askCancel';
aYes.id = 'askYes';
askRow.append(el('i', 'ask-gap'), aCancel, aYes);
askBox.append(askTitle, askRow);
ask.appendChild(askBox);
document.body.appendChild(ask);
let askDo = null;
function askIt(title, yes, then) {
  askTitle.textContent = title;
  aCancel.textContent = T('取消');
  aYes.textContent = T(yes);
  askDo = then;
  ask.classList.add('on');
  aYes.focus();
}
const hideAsk = () => {
  ask.classList.remove('on');
  askDo = null;
};
aCancel.addEventListener('click', () => {
  sound();
  hideAsk();
});
aYes.addEventListener('click', () => {
  sound();
  const d = askDo;
  hideAsk();
  if (d) d();
});
ask.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Escape') {
    sound();
    hideAsk();
  }
});
// a tool that isn't here: the tools page, where its row shows the download. The Mac installs no single tool (all of them
// come in DotAny's folder, and one taken out stays out), so there it only says the tool couldn't be opened
function askInstall(t) {
  if (platform === 'mac') {
    say(couldNotStart(t));
    return;
  }
  askIt(T('先安装 DotAny {name}？', { name: t.name }), '安装', () => {
    if (!solo) go('tools');
    getUpdate(t.key);
  });
}
function askClear() {
  askIt(T('确定清空最近打开记录？'), '清空', () => {
    recents = [];
    hub.clear();
    if (reduce || !recentsEl) {
      fillRecents();
      return;
    }
    morph(recentsEl, fillRecents);
  });
}

// ---------- tools: what is on this computer, read from the disk, and what the latest release has ----------
// The Rust side checks and downloads (src-tauri/src/update.rs); this page says how far it is. One download, the whole
// installer, serves every Update and Install, so every row it is for shows the same progress.
let upd = { state: 'idle', newer: [], offered: [], parts: [], pct: 0, version: '' };
// the Mac: Update found DotAny programs open that it would replace; it waits, still ready, until they are closed (as
// Windows' installer asks in its update mode)
const openWord = () =>
  platform === 'mac' && upd.state === 'ready' && upd.open && upd.open.length
    ? T('请先关闭 {names}。', { names: upd.open.join(T('、')) })
    : '';
let toolsBody = null,
  toolsNote = null,
  toolsCheck = null;
const HUB_ROW = { key: 'hub', name: 'DotAny', ic: 'home', note: '' };
const working = () => upd.state === 'checking' || upd.state === 'downloading' || upd.state === 'installing';
const newerOf = (k) => upd.newer.find((n) => n.id === k);
const inWork = (k) => (upd.state === 'downloading' || upd.state === 'installing') && upd.parts.includes(k);
// the line under the title: what the last check or download came to
function toolsWord() {
  const pct = Math.round(upd.pct * 100);
  return (
    {
      checking: T('正在检查…'),
      latest: T('已是最新版本'),
      newer: T('{n} 个工具有新版本', { n: upd.newer.length }),
      downloading: T('正在下载 {pct}%', { pct }),
      installing: T('正在安装…'),
      // downloaded and checked by itself, waiting for Update
      ready: openWord() || T('新版本已就绪'),
      failed: T('校验失败，下载的文件已删除。'),
      offline: T('网络不可用，未能连接 GitHub。'),
      unreadable: T('未能读取更新信息。'),
    }[upd.state] || ''
  );
}
// the Mac has no Available (可获取): every tool comes in DotAny's folder, and one taken out of it is the person's choice
const groupsOf = () => [
  ['可更新', [...(newerOf('hub') ? [HUB_ROW] : []), ...TOOLS.filter((t) => t.state === 'in' && newerOf(t.key))]],
  ['已安装', TOOLS.filter((t) => t.state === 'in' && !newerOf(t.key))],
  ...(platform === 'mac' ? [] : [['可获取', TOOLS.filter((t) => t.state === 'off')]]),
];
// a part being downloaded shows how far, with a progress line along its foot
function toolRow(t) {
  const row = el('div', 'tool');
  row.id = 'tool-' + t.key;
  row.innerHTML = icon(t.ic);
  const up = newerOf(t.key),
    words = el('span', 'tool-t');
  words.append(el('b', '', t.name), el('small', '', up ? `${up.from} → ${up.to}` : T(t.note)));
  row.appendChild(words);
  if (inWork(t.key)) {
    const n = el('span', 'tool-side', Math.round(upd.pct * 100) + '%'),
      line = el('span', 'tool-line'),
      f = el('i');
    f.style.width = upd.pct * 100 + '%';
    line.appendChild(f);
    row.append(n, line);
  } else if (t.state === 'in' && !up) row.appendChild(el('span', 'tool-side', t.ver));
  else {
    const b = btn('soft', up ? '更新' : '安装', (up ? 'update-' : 'install-') + t.key, () => {
      sound();
      // what is downloaded and checked goes in now; before that, Update downloads and installs it
      if (upd.state === 'ready') installUpdate();
      else getUpdate(up ? null : t.key);
    });
    // Install only for a tool the latest release has, once a check has said which
    b.disabled = working() || (!up && upd.offered.length > 0 && !upd.offered.includes(t.key));
    row.appendChild(b);
  }
  return row;
}
function fillTools() {
  toolsBody.textContent = '';
  groupsOf().forEach(([label, list]) => {
    if (!list.length) return;
    const g = el('section'),
      l = el('div', 'table');
    g.append(el('p', 'label', T(label)), l);
    list.forEach((t) => l.appendChild(toolRow(t)));
    toolsBody.appendChild(g);
  });
}
function drawTools() {
  toolsNote = el('p', 'tool-note-line dyn', toolsWord());
  toolsNote.id = 'toolsNote';
  toolsBody = el('div', 'pg-in');
  toolsBody.style.cssText = 'width:100%;padding:0;margin:0';
  toolsCheck = btn('soft', '检查更新', 'checkTools', () => {
    sound();
    checkUpdate();
  });
  toolsCheck.disabled = working();
  const top = el('div');
  top.append(pageHead('工具', toolsCheck), toolsNote);
  pageIn.append(top, toolsBody);
  shownRows = rowsKey();
  fillTools();
}
// the number of parts with a newer version, beside Tools in the column
function syncToolCount() {
  const b = placeBtns[2],
    n = upd.newer.length;
  let c = b.querySelector('.nav-n');
  if (!n) {
    if (c) c.remove();
    return;
  }
  if (!c) {
    c = el('span', 'nav-n');
    b.appendChild(c);
  }
  c.textContent = String(n);
}
// a new state: the words that changed fade in; the rows move only when which group a part is in, or its button, changes
let shownRows = '';
const rowsKey = () =>
  JSON.stringify([
    TOOLS.map((t) => t.state + t.ver),
    upd.newer,
    upd.offered,
    working(),
    upd.state === 'downloading' || upd.state === 'installing' ? upd.parts : [],
  ]);
function showUpdate(s) {
  const was = upd.state;
  upd = s;
  syncToolCount();
  if (place === 'tools' && toolsBody && toolsBody.isConnected) {
    const word = toolsWord();
    if (toolsNote.textContent !== word) {
      toolsNote.textContent = word;
      if (was !== s.state) {
        toolsNote.classList.remove('swap');
        void toolsNote.offsetWidth;
        toolsNote.classList.add('swap');
      }
    }
    toolsCheck.disabled = working();
    const key = rowsKey();
    if (key !== shownRows) {
      shownRows = key;
      morph(toolsBody, fillTools);
    } else if (upd.state === 'downloading')
      [HUB_ROW, ...TOOLS].forEach((t) => {
        const r = document.getElementById('tool-' + t.key);
        if (!r || !inWork(t.key) || !r.querySelector('.tool-line')) return;
        r.querySelector('.tool-side').textContent = Math.round(upd.pct * 100) + '%';
        r.querySelector('.tool-line i').style.width = upd.pct * 100 + '%';
      });
  }
  renderAbout(was !== s.state);
}
async function checkUpdate() {
  if (working()) return;
  showUpdate(await hub.updateCheck());
}
async function getUpdate(key) {
  if (working()) return;
  showUpdate(await hub.updateGet(key));
}
async function installUpdate() {
  if (working()) return;
  showUpdate(await hub.updateInstall());
}
be.on('update', (s) => showUpdate(s));
// a tool added or taken away beside DotAny shows the next time it looks
async function refreshTools() {
  const was = JSON.stringify(TOOLS);
  await loadTools();
  if (JSON.stringify(TOOLS) === was) return;
  if (place === 'tools' && toolsBody) {
    shownRows = rowsKey();
    morph(toolsBody, fillTools);
  } else if (place === 'home' || place === 'defaults') paintPage();
}

// ---------- default apps. Windows only lets a person change these in its own settings, so the page chooses and Windows confirms ----------
// what Windows has now (told), and what the page has chosen since (defaults). The Mac lets DotAny choose: each switch and
// chip acts at once, and the page shows what Launch Services then says
const mac = platform === 'mac';
let told = new Map(),
  defaults = new Map();
// the Mac: these kinds go to DotAny (on) or back to what opened them before, now; the page shows what the system says after
async function chooseNow(exts, on) {
  exts.forEach((x) => defaults.set(x, on));
  syncDefaults();
  told = new Map(Object.entries(await hub.defaultSet(exts, on)));
  defaults = new Map(told);
  if (defaultsWord && defaultsWord.isConnected) syncDefaults();
}
let defaultsBody = null,
  defaultsWord = null,
  defaultsAll = null,
  defaultRows = [];
async function readDefaults() {
  told = new Map(Object.entries(await hub.defaultApps()));
  defaults = new Map(told);
}
function syncDefaults() {
  let on = 0,
    all = 0;
  defaultRows.forEach(({ exts, sw, chips }) => {
    const n = exts.filter((x) => defaults.get(x)).length;
    on += n;
    all += exts.length;
    sw.setAttribute('aria-checked', String(n === exts.length));
    chips.forEach((b, i) => b.setAttribute('aria-pressed', String(!!defaults.get(exts[i]))));
  });
  const word = T('{all} 种格式，{on} 种用 DotAny 打开', { all, on });
  if (defaultsWord.textContent !== word) {
    defaultsWord.textContent = word;
    if (!reduce) defaultsWord.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  }
  defaultsAll.disabled = on === all;
}
function drawDefaults() {
  defaultsWord = el('p', 'def-word');
  defaultsBody = el('div', 'table');
  defaultRows = [];
  defaultsAll = btn('soft', '全选', 'defaultsAll', () => {
    sound();
    const all = [...defaults.keys()].filter((x) => toolOf(EXT_TOOL[x]) && toolOf(EXT_TOOL[x]).state !== 'off');
    if (mac) {
      chooseNow(all, true);
      return;
    }
    all.forEach((x) => defaults.set(x, true));
    syncDefaults();
  });
  TOOLS.filter((t) => t.state !== 'off').forEach((t) => {
    const exts = t.exts.filter((x) => EXT_TOOL[x] === t.key);
    if (!exts.length) return;
    const row = el('div', 'def'),
      sw = el('button', 'row compact');
    row.innerHTML = icon(t.ic);
    sw.type = 'button';
    sw.id = 'def-' + t.key;
    sw.setAttribute('role', 'switch');
    sw.setAttribute('aria-label', t.name);
    sw.innerHTML = '<i class="tog"></i>';
    sw.addEventListener('mousedown', (e) => e.preventDefault());
    sw.addEventListener('click', () => {
      sound();
      const v = sw.getAttribute('aria-checked') !== 'true';
      if (mac) {
        chooseNow(exts, v);
        return;
      }
      exts.forEach((x) => defaults.set(x, v));
      syncDefaults();
    });
    const box = el('div', 'def-chips'),
      chips = exts.map((x) => {
        const b = el('button', 'chip', x);
        b.type = 'button';
        b.id = 'ext-' + x;
        b.addEventListener('mousedown', (e) => e.preventDefault());
        b.addEventListener('click', () => {
          sound();
          if (mac) {
            chooseNow([x], !defaults.get(x));
            return;
          }
          defaults.set(x, !defaults.get(x));
          syncDefaults();
        });
        box.appendChild(b);
        return b;
      });
    if (mac) {
      row.append(el('span', 'def-name', t.name), box, sw);
      defaultsBody.appendChild(row);
      defaultRows.push({ key: t.key, exts, sw, chips });
      return;
    }
    // this tool's own page in Windows' Default apps, where only its types are listed
    const go = barBtn('go-' + t.key, 'external', T('前往 Windows 设置确认'), async () => {
      sound();
      window.__defaultsOpened = await hub.openDefaultApps(t.key);
    });
    row.append(el('span', 'def-name', t.name), box, go, sw);
    defaultsBody.appendChild(row);
    defaultRows.push({ key: t.key, exts, sw, chips });
  });
  const top = el('div');
  top.append(pageHead('默认应用', defaultsAll), defaultsWord);
  defaultsWord.style.marginTop = '12px';
  // the Mac sets each kind at once, and there is no Windows Settings to send the person to: no foot
  if (mac) {
    pageIn.append(top, defaultsBody);
    syncDefaults();
    return;
  }
  const foot = el('div', 'def-foot');
  foot.append(
    el('p', 'def-note', T('此项设置需在 Windows 设置中确认。')),
    // the first tool chosen here differently from what Windows has, at its own page; with none, Windows' whole list
    btn('primary', '前往 Windows 设置确认', 'defaultsGo', async () => {
      sound();
      const next = defaultRows.find(({ exts }) => exts.some((x) => !!defaults.get(x) !== !!told.get(x)));
      window.__defaultsOpened = await hub.openDefaultApps(next && next.key);
    })
  );
  pageIn.append(top, defaultsBody, foot);
  syncDefaults();
}
// back from Windows' settings: the page shows what Windows now says
addEventListener('focus', async () => {
  if (place !== 'defaults' || !defaultsWord || !defaultsWord.isConnected) return;
  await readDefaults();
  syncDefaults();
});

// ---------- settings: for every tool at once ----------
let startSection = null,
  menuSection = null,
  autoStart = false,
  menuOn = false,
  autoDown = false;
function switchRow(id, run) {
  const r = el('button', 'row');
  r.type = 'button';
  r.id = id;
  r.setAttribute('role', 'switch');
  r.innerHTML = '<span class="row-t"><b class="dyn"></b><small class="dyn"></small></span><i class="tog"></i>';
  r.addEventListener('mousedown', (e) => e.preventDefault());
  r.addEventListener('click', run);
  return r;
}
function drawSettings() {
  if (!startSection) {
    // the Mac's own name for what starts at login (System Settings › General › Login Items)
    startSection = mk(os({ windows: '启动', mac: '登录项' }));
    startSection.appendChild(
      switchRow('autoStart', async () => {
        sound();
        const want = !autoStart;
        autoStart = want;
        syncSwitches();
        const now = await hub.autostart(want);
        if (now !== want) say(T(os({ windows: '无法更新开机自启设置。', mac: '无法更新「登录时打开」设置。' })));
        autoStart = now;
        syncSwitches();
      })
    );
    // Windows' right-click menu is the Finder's Quick Actions on the Mac
    // a new version downloads by itself and waits for Update; this turns the download off
    startSection.appendChild(
      switchRow('autoDownload', async () => {
        sound();
        autoDown = !autoDown;
        syncSwitches();
        autoDown = await hub.autoDownload(autoDown);
        syncSwitches();
      })
    );
    menuSection = mk(os({ windows: '系统右键菜单', mac: '快速操作' }));
    menuSection.appendChild(
      switchRow('shellMenu', async () => {
        sound();
        const want = !menuOn;
        menuOn = want;
        syncSwitches();
        const now = await hub.shellMenu(want, T('使用 DotAny 转换'));
        if (now !== want) say(T(os({ windows: '无法更新系统右键菜单设置。', mac: '无法更新快速操作设置。' })));
        menuOn = now;
        syncSwitches();
      })
    );
  }
  const words = (s, b, small) => {
    const r = s.querySelector('.row');
    r.querySelector('b').textContent = T(b);
    r.querySelector('small').textContent = T(small);
  };
  words(
    startSection,
    os({ windows: '开机时自动启动 DotAny', mac: '登录时打开 DotAny' }),
    // the same Chinese; a language whose Windows sentence names Windows says it without on the Mac
    os({
      windows: '随系统在后台静默就绪，让各款工具即点即开。',
      mac: '随系统在后台静默就绪，让各款工具即点即开。::Mac',
    })
  );
  const down = startSection.querySelector('#autoDownload');
  if (down) {
    down.querySelector('b').textContent = T('自动下载更新');
    down.querySelector('small').textContent = T('检测到新版本时在后台自动下载。');
  }
  words(
    menuSection,
    os({ windows: '集成至系统右键菜单', mac: '集成至访达快速操作' }),
    os({
      windows: '在文件资源管理器的右键菜单中添加「使用 DotAny 转换」（仅对当前账户生效）。',
      mac: '在访达的「快速操作」中添加「使用 DotAny 转换」。',
    })
  );
  syncSwitches();
  const list = [appearance, soundPage, language, startSection, menuSection];
  list.forEach((s) => {
    s.hidden = false;
    translate(s);
  });
  pageIn.append(pageHead('设置'), ...list);
  syncSet();
  // Windows' own state, read again each time: someone may have changed it there
  Promise.all([hub.autostart(), hub.shellMenu(), hub.autoDownload()]).then(([a, m, d]) => {
    autoStart = a;
    menuOn = m;
    autoDown = d;
    syncSwitches();
  });
}
function syncSwitches() {
  if (!startSection) return;
  startSection.querySelector('.row').setAttribute('aria-checked', String(autoStart));
  menuSection.querySelector('.row').setAttribute('aria-checked', String(menuOn));
  const down = startSection.querySelector('#autoDownload');
  if (down) down.setAttribute('aria-checked', String(autoDown));
}

// the old content fades out first; then the box takes its new height while the new content fades in. A newer swap cancels an older one
const OUT = { duration: 140, easing: EASE, fill: 'forwards' };
function swapBox(box, change, after) {
  const n = (box._swap = (box._swap || 0) + 1);
  if (reduce) {
    change();
    if (after) after();
    return;
  }
  box.getAnimations().forEach((a) => a.cancel());
  const old = [...box.children],
    from = box.offsetHeight;
  box.style.height = from + 'px';
  box.style.overflow = 'hidden';
  // the change waits on a timer, not on the fade, so a window in the background still changes on time
  const outs = old.map((c) => c.animate([{ opacity: getComputedStyle(c).opacity }, { opacity: 0 }], OUT));
  wait(OUT.duration).then(() => {
    if (box._swap !== n) return;
    // an element that stays through the change must not keep the fade it just went out with
    change();
    outs.forEach((a) => a.cancel());
    box.style.height = '';
    const to = box.offsetHeight;
    box.style.height = from + 'px';
    if (after) after();
    [...box.children].forEach((c) => c.animate([{ opacity: 0 }, { opacity: 1 }], MOVE));
    const a = box.animate([{ height: from + 'px' }, { height: to + 'px' }], MOVE);
    a.onfinish = a.oncancel = () => {
      if (box._swap === n) {
        box.style.height = '';
        box.style.overflow = '';
      }
    };
  });
}

// ---------- the converter: a box you drop a file into, which opens into the converter itself. The right-click menu opens the same box as a window of its own ----------
const conv = el('section', 'cx');
conv.id = 'convertCard';
const dial = el('div', 'seg cx-dial'),
  dialPl = el('i', 'pl');
dial.id = 'formatList';
dial.appendChild(dialPl);
dial.tabIndex = 0;
const rows = el('div', 'cx-opts'),
  estEl = el('b', 'cx-v dyn'),
  noteEl = el('p', 'cx-note dyn'),
  rule = el('div', 'cx-rule'),
  ruleFill = el('i');
rule.appendChild(ruleFill);
const goBtn = el('button', 'btn primary big', '转换');
goBtn.type = 'button';
goBtn.id = 'convertGo';
goBtn.addEventListener('mousedown', (e) => e.preventDefault());
const sizeL = el('span', 'cx-l', '预计体积'),
  noteBox = el('div', 'cx-notes');
noteBox.appendChild(noteEl);
let files = [],
  fmts = [],
  fmtKey = '',
  fam = '',
  quality = 85,
  tone = 'high',
  packLevel = 'normal',
  busy = false,
  fmtBtns = [],
  estBytes = 0;
const totalSize = () => files.reduce((a, f) => a + (f.size || 0), 0);
const choices = () => ({ quality, tone, pack: packLevel });
let estAt = 0;
function swapText(e, text) {
  if (e.textContent === text) return;
  e.textContent = text;
  const now = performance.now();
  if (now - estAt > 200) {
    e.classList.remove('swap');
    void e.offsetWidth;
    e.classList.add('swap');
  }
  estAt = now;
}
// the size, the way the save card says "About X"; what a format leaves behind goes on a line of its own under the band
// while the box is being rebuilt the note is set at once, so the box measures the height it will really have
let estSeq = 0;
// workbooks with sheets of their own, and pictures that can be partly see-through
const WORKBOOK = ['xlsx', 'xlsm', 'xls', 'ods'],
  SEE_THROUGH = ['png', 'webp', 'gif', 'ico', 'tif', 'tiff'];
function estimate(moving) {
  if (!files.length || busy) return;
  const seq = ++estSeq;
  hub
    .estimate(
      files.map((f) => f.path),
      fmtKey,
      choices()
    )
    .then((b) => {
      if (seq !== estSeq || busy || !files.length) return;
      estBytes = b;
      swapText(estEl, money(b));
    });
  let note = '';
  if (fmtKey === 'stl') note = T('仅保留三维几何网格，不包含色彩与动画。');
  else if (fmtKey === 'obj') note = T('导出后不包含动画与材质贴图。');
  else if (
    fam === 'table'
    && (fmtKey === 'csv' || fmtKey === 'tsv')
    && files.some((f) => WORKBOOK.includes(extOf(f.name)))
  )
    note = T('仅导出首个工作表的数据，不包含排版样式。');
  else if (fmtKey === 'png' && fam === 'pdf') note = T('每一页将分别导出为独立图像。');
  else if (fmtKey === 'jpg' && files.some((f) => SEE_THROUGH.includes(extOf(f.name))))
    note = T('透明区域将填充为白色。');
  if (noteEl.textContent !== note) {
    if (moving && noteEl.isConnected && !reduce)
      swapBox(noteBox, () => {
        noteEl.textContent = note;
      });
    else noteEl.textContent = note;
  }
}
function chipRow(name, opts, get, set) {
  const box = el('div'),
    chips = el('div', 'chips three');
  opts.forEach(([k, t]) => {
    const b = el('button', 'chip', T(t));
    b.type = 'button';
    b.id = 'opt-' + k;
    b.setAttribute('aria-pressed', String(k === get()));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      set(k);
      [...chips.children].forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      estimate();
    });
    chips.appendChild(b);
  });
  box.append(el('span', 'side-label dyn', T(name)), chips);
  return box;
}
function qualitySlider() {
  const w = el('label', 'slide'),
    top = el('span', 'slide-top'),
    val = el('span', 'dyn', String(quality));
  top.append(el('b', 'dyn', T('品质')), val);
  const i = document.createElement('input');
  i.type = 'range';
  i.id = 'convertQuality';
  i.min = 10;
  i.max = 100;
  i.value = quality;
  i.setAttribute('aria-label', T('品质'));
  const paint = () => {
    i.style.setProperty('--a', '0%');
    i.style.setProperty('--b', ((quality - 10) / 90) * 100 + '%');
    val.textContent = String(quality);
  };
  i.addEventListener('input', () => {
    quality = +i.value;
    paint();
    estimate();
  });
  paint();
  w.append(top, i);
  return w;
}
// the converter's chips: one whose words don't fit its column takes two (parts.js fitChips), as in every tool
let chipsFitting = 0;
const refitChips = () => {
  if (!chipsFitting)
    chipsFitting = requestAnimationFrame(() => {
      chipsFitting = 0;
      fitChips(document.getElementById('main') || document);
    });
};
new ResizeObserver(refitChips).observe(document.documentElement);
function drawOptions() {
  refitChips();
  rows.textContent = '';
  if (fmtKey === 'jpg' || fmtKey === 'webp') rows.appendChild(qualitySlider());
  else if (fmtKey === 'mp3')
    rows.appendChild(
      chipRow(
        '音质',
        [
          ['normal', '标准'],
          ['high', '高'],
          ['top', '极高'],
        ],
        () => tone,
        (k) => {
          tone = k;
        }
      )
    );
  else if (fmtKey === 'zip' || fmtKey === '7z')
    rows.appendChild(
      chipRow(
        '压缩率',
        [
          ['fast', '快速'],
          ['normal', '标准'],
          ['small', '极致体积'],
        ],
        () => packLevel,
        (k) => {
          packLevel = k;
        }
      )
    );
}
function placeDial(slide) {
  const on = fmtBtns.find((b) => b.dataset.k === fmtKey);
  fmtBtns.forEach((b) => b.setAttribute('aria-pressed', String(b === on)));
  if (!on) return;
  if (!slide) dialPl.style.transition = 'none';
  dialPl.style.transform = `translateX(${on.offsetLeft}px)`;
  dialPl.style.width = on.offsetWidth + 'px';
  if (!slide) {
    void dialPl.offsetWidth;
    dialPl.style.transition = '';
  }
}
dial.addEventListener('keydown', (e) => {
  if (!fmts.length || busy) return;
  if (e.key === 'Enter') {
    e.preventDefault();
    runConvert();
    return;
  }
  const at = fmts.indexOf(fmtKey),
    to = { ArrowLeft: at - 1, ArrowRight: at + 1, Home: 0, End: fmts.length - 1 }[e.key];
  if (to == null) return;
  e.preventDefault();
  const k = fmts[Math.max(0, Math.min(fmts.length - 1, to))];
  if (k !== fmtKey) {
    sound();
    setFmt(k);
  }
});
function setFmt(k) {
  if (k === fmtKey) return;
  const had = optionsFor(fmtKey);
  fmtKey = k;
  placeDial(true);
  if (had !== optionsFor(k) || had === 'chips') swapBox(rows, drawOptions);
  estimate(true);
}
// formats that share an option keep it on screen as it is
const optionsFor = (k) =>
  k === 'jpg' || k === 'webp' ? 'quality' : k === 'mp3' ? 'chips' : k === 'zip' || k === '7z' ? 'chips' : '';

// what the box holds: empty, a place to drop; full, the file, the formats, and the button
function fillConv() {
  conv.textContent = '';
  conv.classList.toggle('full', !!files.length);
  if (!files.length) {
    conv.innerHTML = icon('file');
    conv.append(
      el('p', 'cx-drop', T('拖拽文件至此')),
      btn('soft', '打开本地文件', 'pickOpen', () => {
        sound();
        pickConvert();
      })
    );
    return;
  }
  const many = files.length > 1,
    head = el('div', 'cx-file');
  head.innerHTML = icon(many ? 'convert' : iconOf(files[0].name));
  const out = el('button', 'cx-out');
  out.type = 'button';
  out.id = 'convertCancel';
  out.setAttribute('aria-label', T('移除'));
  out.innerHTML = icon('close');
  out.addEventListener('mousedown', (e) => e.preventDefault());
  out.addEventListener('click', () => {
    if (busy) return;
    sound();
    stopConvert();
  });
  head.append(
    el('span', 'cx-name', many ? T('{n} 个文件', { n: files.length }) : files[0].name),
    el('span', 'cx-size', money(totalSize())),
    out
  );
  [...dial.querySelectorAll('.sg')].forEach((b) => b.remove());
  fmtBtns = fmts.map((k) => {
    const b = el('button', 'sg', k.toUpperCase());
    b.type = 'button';
    b.dataset.k = k;
    b.id = 'fmt-' + k;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      if (busy) return;
      sound();
      setFmt(k);
    });
    dial.appendChild(b);
    return b;
  });
  ruleFill.style.width = '0';
  goBtn.disabled = false;
  goBtn.textContent = T('转换');
  // one band: the labels share a line, the controls share a line, and the button closes the controls' line
  const band = el('div', 'cx-band'),
    pick = el('div', 'cx-cell'),
    size = el('div', 'cx-cell cx-size-cell');
  pick.append(el('span', 'cx-l', T('转换为')), dial);
  sizeL.textContent = T('预计体积');
  estEl.textContent = money(estBytes);
  size.append(sizeL, estEl);
  band.append(pick, rows, size, goBtn);
  noteEl.textContent = '';
  conv.append(head, rule, band, noteBox);
  estimate();
}
// every change of what the box holds opens or closes it in place, never by redrawing the page
function reshape() {
  if (place !== 'convert' || !conv.isConnected) {
    go('convert', true);
    return;
  }
  swapBox(conv, fillConv, () => placeDial());
}
const convRec = el('div', 'recents');
watchRecCols(convRec);
const recFor = () => recents.filter((r) => !r.gone && r.convertible).slice(0, 6);
const fillConvRec = () => {
  convRec.textContent = '';
  recFor().forEach((r) => convRec.appendChild(recentRow(r, true)));
};
// after a conversion only what changed moves: new files open in at the top, the ones pushed past the end close at the bottom
function growConvRec() {
  const want = recFor(),
    have = new Map([...convRec.children].map((e) => [e.dataset.path, e]));
  if (reduce) {
    fillConvRec();
    return;
  }
  const keep = new Set(want.map((r) => r.path));
  have.forEach((e, n) => {
    if (keep.has(n)) return;
    e.style.pointerEvents = 'none';
    const h = e.offsetHeight;
    e.animate(
      [
        { height: h + 'px', opacity: 1 },
        { height: 0, opacity: 0 },
      ],
      { ...MOVE, fill: 'forwards' }
    );
    wait(MOVE.duration).then(() => e.remove());
  });
  let before = convRec.firstChild;
  want.forEach((r) => {
    const old = have.get(r.path);
    if (old) {
      before = old.nextSibling;
      return;
    }
    const e = recentRow(r, true);
    convRec.insertBefore(e, before);
    const h = e.offsetHeight;
    e.style.overflow = 'hidden';
    e.animate(
      [
        { height: 0, opacity: 0 },
        { height: h + 'px', opacity: 1 },
      ],
      MOVE
    ).onfinish = () => {
      e.style.overflow = '';
    };
  });
}
function drawConvert() {
  fillConv();
  const items = [pageHead('转换'), conv];
  if (!solo) {
    const rec = el('section');
    fillConvRec();
    rec.append(el('p', 'label', T('最近打开')), convRec);
    if (convRec.children.length) items.push(rec);
  }
  pageIn.append(...(solo ? [conv] : items));
  placeDial();
  if (files.length && !busy)
    setTimeout(() => {
      if (place === 'convert' && dial.isConnected) dial.focus({ preventScroll: true });
    }, 30);
}
const REFUSED = {
  video: '暂不支持转换视频文件。',
  mixed: '所选文件类型不同，无法批量转换。',
  unsupported: '暂不支持转换此类文件。',
};
let asking = 0;
async function startConvert(fileList) {
  if (busy || !fileList.length) return;
  const seq = ++asking;
  const t = await hub.targets(fileList.map((f) => f.path));
  if (seq !== asking || busy) return;
  if (t.refused) {
    say(T(REFUSED[t.refused] || REFUSED.unsupported));
    return;
  }
  const first = t.formats[0];
  estBytes = await hub.estimate(
    fileList.map((f) => f.path),
    first,
    choices()
  );
  if (seq !== asking || busy) return;
  files = fileList;
  fmts = t.formats;
  fam = t.family;
  fmtKey = first;
  drawOptions();
  conv.classList.remove('busy');
  reshape();
}
function stopConvert() {
  if (busy) return;
  asking++;
  files = [];
  fmts = [];
  fmtKey = '';
  rows.textContent = '';
  reshape();
}
let progressAt = 0;
be.on('convert-progress', (p) => {
  if (!busy) return;
  const pct = Math.min(100, p * 100);
  ruleFill.style.width = pct + '%';
  // words that change every fraction of a second don't animate
  estEl.textContent = Math.round(pct) + '%';
  progressAt = performance.now();
});
// why a conversion stopped (dotany-convert's reason), in the family's words; what was finished before it stays
function whyNot(r) {
  const n = r.file || '',
    why = r.reason || {};
  const words = {
    refused: () => T(REFUSED[why.detail] || REFUSED.unsupported),
    damaged: () => T('「{name}」已损坏，未能转换。', { name: n }),
    unreadable: () => T('无法读取「{name}」，未能转换。', { name: n }),
    locked: () => T('「{name}」有密码，未能转换。', { name: n }),
    unwritable: () => T('原文件所在目录缺少写入权限，无法完成转换。'),
    toolarge: () => T('「{name}」体积过大，未能转换。', { name: n }),
    missing: () => T('DotAny 缺少 {part}，未能转换。', { part: why.detail }),
  }[why.kind];
  let text = words ? words() : T('转换未能完成，原文件未受影响。');
  const kept = (r.files || []).length;
  if (kept) text += T('已转换的 {n} 个文件已保存至原文件所在目录。', { n: kept });
  return text;
}
async function runConvert() {
  if (busy || !files.length) return;
  sound();
  busy = true;
  goBtn.disabled = true;
  conv.classList.add('busy');
  swapText(sizeL, T('正在转换'));
  estEl.textContent = '0%';
  const list = files.map((f) => f.path),
    target = fmtKey;
  // a Word file's pictures go beside its Markdown, named in the language on screen: 报告 图1.png, Report Figure 1.png;
  // the words a converted document carries for what it can't hold are in that language too
  const r = await hub.convert(list, target, { ...choices(), picture: T('图{n}'), lang });
  ruleFill.style.width = '100%';
  await wait(reduce ? 0 : 260);
  busy = false;
  conv.classList.remove('busy');
  if (r.status !== 'done') {
    goBtn.disabled = false;
    ruleFill.style.width = '0';
    swapText(sizeL, T('预计体积'));
    estimate();
    say(whyNot(r));
    if (r.files && r.files.length) {
      await loadRecents();
      if (convRec.isConnected) growConvRec();
    }
    return;
  }
  const out = r.files.map((p) => p.replace(/^.*[\\/]/, '')),
    several = out.length > 1;
  await loadRecents();
  files = [];
  fmts = [];
  fmtKey = '';
  rows.textContent = '';
  if (place === 'convert' && conv.isConnected) {
    reshape();
    if (convRec.isConnected) growConvRec();
    else if (!solo && recFor().length) go('convert', true);
  } else if (place === 'convert') go('convert', true);
  say(
    (several ? T('已转换 {n} 个文件。', { n: out.length }) : T('已转换 {name}。', { name: out[0] }))
      + T('新文件将保存在原文件同级目录下，原文件保持不变。')
  );
}
addEventListener('keydown', (e) => {
  if (
    e.key === 'Escape'
    && place === 'convert'
    && files.length
    && !busy
    && !document.querySelector('.ask-wrap.on, .new-wrap.on')
  ) {
    sound();
    stopConvert();
  }
});
goBtn.addEventListener('click', runConvert);
// the right-click menu starts one DotAny per file chosen; those that come within a moment join this window
const soloPaths = [...(boot.files || [])];
async function takeMore(ready) {
  const more = (await hub.waiting(ready)).filter((p) => !soloPaths.includes(p));
  soloPaths.push(...more);
  if (ready || more.length) startConvert(await hub.files(soloPaths));
}
be.on('convert-add', () => {
  if (solo && !busy) takeMore(false);
});

// ---------- contact: written like the top of a letter. The address in large type, and a line to write on ----------
// the address people can write to; the button beside it copies it
const MAIL = 'contact@dotany.app';
// what the line beside Send says, kept while DotAny runs: received, or why it was not sent
let fbNote = '';
// the line for each answer of send_feedback; any other answer is "Couldn’t send." (the text then stays in the box)
const FB_NOTE = { sent: '已收到，感谢反馈。', offline: '网络不可用，未能发送。' };
function drawContact() {
  // copying is an action, so it is the family's icon button; it answers with a tick for a moment
  const mailBox = el('section', 'ct-mail'),
    addr = el('p', 'ct-addr', MAIL),
    copy = el('button', 'ct-copy');
  copy.type = 'button';
  copy.id = 'contactCopy';
  // the old icon fades out, the new one comes in with a small lift
  const showCopy = (ic, t, now) => {
    copy.setAttribute('aria-label', T(t));
    copy.dataset.tip = T(t);
    const was = copy.querySelector('svg');
    const put = () => {
      copy.innerHTML = icon(ic);
      if (!now && !reduce)
        copy.querySelector('svg').animate(
          [
            { opacity: 0, transform: 'scale(.82)' },
            { opacity: 1, transform: 'none' },
          ],
          MOVE
        );
    };
    if (now || reduce || !was) {
      put();
      return;
    }
    was.animate([{ opacity: 1 }, { opacity: 0 }], OUT);
    wait(OUT.duration).then(put);
  };
  // the word for copying to the clipboard is 复制 on Windows and 拷贝 in the Mac's own Chinese
  const COPY = os({ windows: '复制', mac: '拷贝' });
  showCopy('copy', COPY, true);
  copy.addEventListener('mousedown', (e) => e.preventDefault());
  copy.addEventListener('click', async () => {
    sound();
    try {
      await navigator.clipboard.writeText(MAIL);
    } catch {}
    showCopy('tick', os({ windows: '已复制', mac: '已拷贝' }));
    copy.classList.add('done');
    clearTimeout(copy.t);
    copy.t = setTimeout(() => {
      if (!copy.isConnected) return;
      copy.classList.remove('done');
      showCopy('copy', COPY);
    }, 2000);
  });
  const addrLine = el('div', 'ct-line');
  addrLine.append(addr, copy);
  mailBox.append(el('p', 'label', T('电子邮件')), addrLine);
  const fb = el('section', 'ct-fb'),
    text = document.createElement('textarea');
  text.id = 'feedbackText';
  text.rows = 4;
  text.spellcheck = false;
  text.placeholder = T('欢迎反馈使用问题或功能建议。');
  // as many as the receiver keeps (feedback.rs, MAX_CHARS)
  text.maxLength = 4000;
  const done = el('p', 'ct-done dyn', fbNote ? T(fbNote) : '');
  // sent to DotAny's receiver; what was written fades away rather than vanishing. When it was not sent, it stays (and
  // waits in DotAny's folder until it is), and the line says so
  const send = btn('primary big', '发送', 'feedbackSend', async () => {
    sound();
    send.disabled = true;
    text.readOnly = true;
    const [sent] = await Promise.all([hub.feedback(text.value, lang), wait(reduce ? 0 : 400)]);
    if (sent !== 'sent') {
      text.readOnly = false;
      send.disabled = !text.value.trim();
      fbNote = FB_NOTE[sent] || '未能发送。';
      swapText(done, T(fbNote));
      return;
    }
    const gone = reduce ? null : text.animate([{ opacity: 1 }, { opacity: 0 }], OUT);
    if (gone) await wait(OUT.duration);
    text.value = '';
    if (gone) gone.cancel();
    text.readOnly = false;
    if (!reduce) text.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
    fbNote = FB_NOTE.sent;
    swapText(done, T(fbNote));
  });
  const calm = () => {
    if (!fbNote) return;
    fbNote = '';
    if (reduce) {
      done.textContent = '';
      return;
    }
    const a = done.animate([{ opacity: 1 }, { opacity: 0 }], OUT);
    wait(OUT.duration).then(() => {
      if (!fbNote) done.textContent = '';
      a.cancel();
    });
  };
  text.addEventListener('input', () => {
    send.disabled = !text.value.trim();
    calm();
  });
  send.disabled = true;
  // a message that could not be sent is back in the box
  hub.feedbackDraft().then((t) => {
    if (!t || text.value || !text.isConnected) return;
    text.value = t;
    send.disabled = !t.trim();
  });
  text.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter' && modKey(e)) {
      e.preventDefault();
      if (!send.disabled) send.click();
    }
  });
  const foot = el('div', 'ct-foot');
  foot.append(done, send);
  fb.append(el('p', 'label', T('意见反馈')), text, foot);
  pageIn.append(pageHead('联系我们'), mailBox, fb);
}

// ---------- about: the family's about page, as it is ----------
function drawAbout() {
  translate(about);
  about.hidden = false;
  const h = about.querySelector('h2');
  if (h) h.hidden = true;
  pageIn.append(pageHead('关于'), about);
}

// ---------- files from outside: two places to put them ----------
// what can be opened goes to its tool; the picker shows every kind of file the family opens
async function openChosen(paths) {
  const p = paths[0];
  if (!p) return;
  if (!EXT_TOOL[extOf(p)]) {
    say(T('暂不支持该文件格式。'));
    return;
  }
  openWith(p, '');
}
async function pickOpen() {
  const p = await hub.pick(T('文件'), false);
  if (p.length) openChosen(p);
}
async function pickConvert() {
  const p = await hub.pick(T('文件'), true);
  if (p.length) startConvert(await hub.files(p));
}
const drop2 = el('div', 'drop2'),
  dropPair = el('div', 'drop-pair'),
  dropPl = el('i', 'drop-pl'),
  zOpen = el('div', 'drop-z on'),
  zConv = el('div', 'drop-z');
zOpen.innerHTML = icon('doc');
zConv.innerHTML = icon('convert');
zOpen.append(el('p', '', '打开'));
zConv.append(el('p', '', '转换'));
dropPair.dataset.at = 'open';
dropPair.append(dropPl, zOpen, zConv);
drop2.appendChild(dropPair);
document.body.appendChild(drop2);
const dropAt = (k) => {
  if (dropPair.dataset.at === k) return;
  dropPair.dataset.at = k;
  zOpen.classList.toggle('on', k === 'open');
  zConv.classList.toggle('on', k === 'convert');
};
// Windows hands the files over, not the page: the window says where they are while they come
const sideOf = (p) =>
  p && p.position ? (p.position.x / devicePixelRatio < innerWidth / 2 ? 'open' : 'convert') : null;
be.onWebview('tauri://drag-enter', (p) => {
  dropAt(place === 'convert' ? 'convert' : 'open');
  drop2.classList.add('on');
});
be.onWebview('tauri://drag-over', (p) => {
  const s = sideOf(p);
  if (s) dropAt(s);
});
be.onWebview('tauri://drag-leave', () => drop2.classList.remove('on'));
be.onWebview('tauri://drag-drop', async (p) => {
  drop2.classList.remove('on');
  const paths = (p && p.paths) || [];
  if (!paths.length) return;
  if (dropPair.dataset.at === 'convert') startConvert(await hub.files(paths));
  else openChosen(paths);
});

// ---------- settings: the family's pages, here one under another ----------
const face = (v) =>
  '<span class="face face-'
  + v
  + '"><i class="top"><i class="seg-m"></i></i><i class="h"></i><i class="l"></i><i class="l s"></i><i class="cr"></i></span>';
const preview = (v) => '<span class="pv">' + (v === 'system' ? face('light') + face('dark') : face(v)) + '</span>';
const applyTheme = () => {
  const t = store.get('dotany-theme');
  if (t === 'light' || t === 'dark') root.dataset.theme = t;
  else delete root.dataset.theme;
  syncFrame();
};
const mk = (title) => {
  const p = el('section', 'set-pg');
  p.appendChild(el('h2', '', title));
  return p;
};
const appearance = mk('外观'),
  cards = el('div', 'cards');
const themeCards = [
  ['system', '跟随系统'],
  ['light', '浅色'],
  ['dark', '深色'],
].map(([v, t]) => {
  const b = el('button', 'card');
  b.type = 'button';
  b.dataset.v = v;
  b.innerHTML = preview(v) + '<span>' + t + '</span>';
  b.addEventListener('click', () => {
    sound();
    store.set('dotany-theme', v === 'system' ? null : v);
    applyTheme();
    syncSet();
  });
  cards.appendChild(b);
  return b;
});
appearance.appendChild(cards);
const soundPage = mk('声音'),
  soundRow = el('button', 'row');
soundRow.type = 'button';
soundRow.setAttribute('role', 'switch');
soundRow.id = 'soundRow';
soundRow.innerHTML =
  '<span class="row-t"><b>交互音效</b><small>触发按钮操作时播放提示音。</small></span><i class="tog"></i>';
soundRow.addEventListener('click', () => {
  store.set('dotany-sound', store.get('dotany-sound') === 'on' ? null : 'on');
  sound();
  syncSet();
});
soundPage.appendChild(soundRow);
// Settings › Language (语言): one row and the family's menu, as every tool's settings have it (shared/web/parts.js)
const language = mk('语言');
const langMenu = el('div', 'pop');
langMenu.id = 'langMenu';
document.body.appendChild(langMenu);
let langMenuFor = null;
const closeLangMenu = () => {
  if (!langMenuFor) return false;
  langMenu.classList.remove('on');
  langMenuFor = null;
  return true;
};
/** The family's menu (.pop) under a button, as shell/file-menu.js's openPop draws it: items { value, label, raw, on } or { sep }. */
function openLangMenu(btn, items, pick) {
  if (langMenuFor === btn) {
    closeLangMenu();
    return;
  }
  langMenu.textContent = '';
  items.forEach((it) => {
    if (it.sep) {
      langMenu.appendChild(el('i', 'pop-sep'));
      return;
    }
    const b = el('button');
    b.type = 'button';
    b.appendChild(el('span', '', it.raw ? it.label : T(it.label)));
    if (it.id) b.id = it.id;
    if (it.raw) b.dataset.raw = '';
    if (it.on) b.setAttribute('aria-pressed', 'true');
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      closeLangMenu();
      pick(it.value);
    });
    langMenu.appendChild(b);
  });
  const r = btn.getBoundingClientRect();
  langMenu.style.left = Math.max(8, Math.min(innerWidth - 8 - langMenu.offsetWidth, r.left)) + 'px';
  langMenu.style.top = r.bottom + 6 + 'px';
  langMenuFor = btn;
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (langMenuFor === btn) langMenu.classList.add('on');
    })
  );
}
document.addEventListener('pointerdown', (e) => {
  if (langMenuFor && !langMenu.contains(e.target) && !langMenuFor.contains(e.target)) closeLangMenu();
});
addEventListener(
  'keydown',
  (e) => {
    if (e.key === 'Escape' && closeLangMenu()) e.stopPropagation();
  },
  true
);
const langRow = languageRow({
  LANGS,
  get: () => store.get('dotany-lang'),
  pick: (v) => {
    store.set('dotany-lang', v);
    applyLang();
    syncSet();
  },
  openPop: openLangMenu,
});
language.appendChild(langRow.row);
const about = el('section', 'set-pg'),
  aboutMain = el('div'),
  aboutLic = el('div');
aboutLic.hidden = true;
aboutMain.appendChild(el('h2', '', '关于'));
const licOpen = el('button', 'btn soft', '开源许可与字体协议');
licOpen.type = 'button';
licOpen.id = 'openLicences';
const updBtn = el('button', 'btn primary upd');
updBtn.type = 'button';
updBtn.id = 'checkUpdate';
const ver = el('p', 'about-ver dyn');
ver.id = 'aboutVer';
// the same check and download as the tools page: the line under the name says where it is, the button what comes next
const ABOUT_WORD = {
  latest: '已是最新版本',
  failed: '校验失败，下载的文件已删除。',
  offline: '网络不可用，未能连接 GitHub。',
  unreadable: '未能读取更新信息。',
  ready: '新版本已就绪',
};
function renderAbout(changed) {
  const st = upd.state,
    word =
      st === 'newer'
        ? T('{n} 个工具有新版本', { n: upd.newer.length })
        : openWord() || (ABOUT_WORD[st] ? T(ABOUT_WORD[st]) : '');
  const text = T('版本 {version}', { version: boot.version || '1.0.0' }) + (word ? ' · ' + word : '');
  if (ver.textContent !== text) {
    ver.textContent = text;
    if (changed && ver.isConnected) {
      ver.classList.remove('swap');
      void ver.offsetWidth;
      ver.classList.add('swap');
    }
  }
  const busy = working(),
    label =
      {
        checking: T('正在检查…'),
        downloading: T('正在下载 {pct}%', { pct: Math.round(upd.pct * 100) }),
        installing: T('正在安装…'),
      }[st] || (upd.newer.length ? T('更新') : T('检查更新'));
  updBtn.classList.toggle('busy', busy);
  if (!!updBtn.querySelector('.pulse') !== busy || !updBtn.lastChild)
    updBtn.innerHTML = busy ? '<i class="pulse"></i><span></span>' : '<span></span>';
  updBtn.lastChild.textContent = label;
}
updBtn.addEventListener('click', () => {
  sound();
  if (working()) return;
  if (upd.state === 'ready') installUpdate();
  else if (upd.newer.length) getUpdate(null);
  else checkUpdate();
});
const ab = el('div', 'about'),
  abBtns = el('div', 'about-btns');
abBtns.append(updBtn, licOpen);
ab.append(el('p', 'about-name', 'DotAny'), ver, abBtns);
aboutMain.appendChild(ab);
const licBack = el('button', 'lic-back');
licBack.type = 'button';
licBack.innerHTML = icon('back');
licBack.setAttribute('aria-label', '返回');
const licHead = el('div', 'lic-head');
licHead.append(licBack, el('h2', '', '开源许可与字体协议'));
const licList = el('div', 'lic-list');
[
  ['DotAny', '本程序', 'MIT'],
  ['HarmonyOS Sans', '中文字体', 'HarmonyOS Sans Fonts License'],
  ['Stack Sans', '西文与数字字体', 'SIL OFL 1.1'],
  ['Noto Sans JP', '日文字体', 'SIL OFL 1.1'],
  ['Pretendard', '韩文字体', 'SIL OFL 1.1'],
  ['Lucide', '图标', 'ISC'],
  ['Tauri', '应用框架', 'MIT / Apache-2.0'],
  ['其他开源组件', '完整清单与协议见下方', 'MIT, Apache-2.0…'],
].forEach(([n, r, l]) => {
  const row = el('div', 'lic-row'),
    left = el('span', 'lic-n');
  left.append(el('b', n === '其他开源组件' ? '' : 'dyn', n), el('small', '', r));
  row.append(left, el('span', 'lic-l dyn', l));
  licList.appendChild(row);
});
const licText = el('pre', 'lic-text dyn', HARMONY_LICENCE);
licText.id = 'licText';
licText.tabIndex = 0;
aboutLic.append(licHead, licList, licText);
about.append(aboutMain, aboutLic);
function showLicences(on) {
  const was = aboutLic.hidden ? aboutMain : aboutLic,
    now = on ? aboutLic : aboutMain;
  [aboutMain, aboutLic].forEach((e) => {
    e.getAnimations().forEach((a) => a.cancel());
    e.classList.remove('leaving-pg');
  });
  aboutMain.hidden = on;
  aboutLic.hidden = !on;
  if (was !== now && !reduce && about.isConnected) {
    was.hidden = false;
    was.classList.add('leaving-pg');
    const out = was.animate(
      [
        { opacity: 1, transform: 'none' },
        { opacity: 0, transform: 'translateX(' + (on ? -12 : 12) + 'px)' },
      ],
      { duration: 160, easing: EASE, fill: 'forwards' }
    );
    out.onfinish = () => {
      was.classList.remove('leaving-pg');
      was.hidden = true;
      out.cancel();
    };
    now.animate(
      [
        { opacity: 0, transform: 'translateX(' + (on ? 16 : -16) + 'px)' },
        { opacity: 1, transform: 'none' },
      ],
      { duration: 320, delay: 50, easing: EASE, fill: 'backwards' }
    );
  }
}
// the long list of every library arrives when the page is first opened
let noticesAdded = false;
licOpen.addEventListener('click', () => {
  sound();
  showLicences(true);
  if (noticesAdded) return;
  noticesAdded = true;
  // the program's own licence first (shared/scripts/own-licence.mjs writes it with its separating line)
  import('./notices.generated.js').then(({ OWN = '', NOTICES: all }) => {
    licText.textContent = OWN + HARMONY_LICENCE + '\n\n' + all;
  });
});
licBack.addEventListener('click', () => {
  sound();
  showLicences(false);
});
function syncSet() {
  const c = store.get('dotany-theme') || 'system';
  themeCards.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === c)));
  soundRow.setAttribute('aria-checked', String(store.get('dotany-sound') === 'on'));
  langRow.sync();
}

// every fixed word translates in place; each element remembers its Chinese
const srcText = new WeakMap(),
  srcAttr = new WeakMap();
function translate(top) {
  const w = document.createTreeWalker(top, NodeFilter.SHOW_TEXT);
  for (let n = w.nextNode(); n; n = w.nextNode()) {
    if (n.parentElement.closest('.name, .dyn, .state, .ext, .warn-t, .upd, .ask-title, [data-raw]')) continue;
    if (!srcText.has(n)) srcText.set(n, n.nodeValue);
    n.nodeValue = T(srcText.get(n));
  }
  [top, ...top.querySelectorAll('[aria-label], [data-tip]')].forEach((e) => {
    const a = e.getAttribute('aria-label'),
      t = e.dataset.tip;
    if (!a && !t) return;
    if (!srcAttr.has(e)) srcAttr.set(e, [a, t]);
    const o = srcAttr.get(e);
    if (o[0]) e.setAttribute('aria-label', T(o[0]));
    if (o[1]) e.dataset.tip = T(o[1]);
  });
}
let shownLang = null;
function applyLang() {
  setLang(pickLang(store.get('dotany-lang')));
  root.lang = langInfo().html;
  // a language's table is loaded the first time it is chosen; until it is here its words are English's
  if (!langReady()) {
    const want = lang;
    loadLang(want).then(() => {
      if (lang === want) applyLang();
    });
  }
  [rail, winBox, drop2, ask, appearance, soundPage, language, about, langMenu].forEach(translate);
  if (startSection) [startSection, menuSection].forEach(translate);
  renderAbout();
  if (shownLang && shownLang !== lang && place === 'settings' && !reduce)
    pageIn.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  const changed = shownLang && shownLang !== lang;
  shownLang = lang;
  if (files.length) drawOptions();
  if (place !== 'settings') paintPage();
  else {
    pageIn.textContent = '';
    drawSettings();
  }
  placePill();
  syncSet();
  if (solo) be.setTitle(T('使用 DotAny 转换'));
  else setWindowTitle('');
  // the right-click menu speaks the language on screen
  if (changed && menuOn) hub.shellMenu(true, T('使用 DotAny 转换'));
}
// appearance, sound and language are the family's: a change in any tool reaches DotAny at once, and the other way round
be.on('shared-settings', (values) => {
  const was = store.get('dotany-lang');
  Object.entries(values).forEach(([k, v]) => store.took(k, v));
  applyTheme();
  if (store.get('dotany-lang') !== was) applyLang();
  else syncSet();
});
// the shared list changed: a tool opened or saved a file, or DotAny converted one
be.on('recents', () => {
  reloadRecents();
});
addEventListener('focus', () => {
  refreshTools();
});

addEventListener('keydown', (e) => {
  const mod = modKey(e),
    k = e.key;
  if (mod && k.toLowerCase() === 'o') {
    e.preventDefault();
    if (solo) pickConvert();
    else pickOpen();
    return;
  }
  if (mod && k.toLowerCase() === 'n' && !solo) {
    e.preventDefault();
    sound();
    openNew('doc');
    return;
  }
  if (k === 'Escape') {
    if (closeNew()) {
      sound();
      return;
    }
  }
});
addEventListener('resize', () => {
  placePill();
  if (files.length && place === 'convert') placeDial();
});

// ---------- begin ----------
(async () => {
  navPl.style.transition = 'none';
  applyTheme();
  await Promise.all([
    loadTools(),
    loadRecents(),
    readDefaults(),
    hub.updateStatus().then((s) => {
      upd = s;
    }),
  ]);
  syncToolCount();
  // opened from the right-click menu: only the converter, with the files already in it
  if (solo) {
    root.classList.add('solo');
    place = 'convert';
  }
  applyLang();
  if (!langReady()) {
    await loadLang();
    applyLang();
  }
  // the files that came while this page was loading join the first, and for a moment more can still come
  if (solo) await takeMore(true);
  void navPl.offsetWidth;
  navPl.style.transition = '';
  if (document.fonts && document.fonts.ready)
    document.fonts.ready.then(() => {
      placePill();
      if (place === 'convert' && files.length) placeDial();
    });
  // a language's own font arrives after its words (繁體中文, 日本語, 한국어 the first time): place what was measured again
  if (document.fonts)
    document.fonts.addEventListener('loadingdone', () => {
      placePill();
      if (place === 'convert' && files.length) placeDial();
    });
  // which HarmonyOS Sans SC the page uses (Windows' or its own, fonts.js), settled before the window shows
  await fontsChosen;
  // a window Windows started quietly stays out of sight until someone opens DotAny
  if (!boot.hidden) await be.windowReady();
  // the layout check shows the window in a made-up language and back, whatever is stored
  if (boot.test)
    window.__lang = async (code) => {
      await testLanguage(code);
      applyLang();
    };
  if (boot.test)
    window.__s = {
      get recents() {
        return recents;
      },
      get place() {
        return place;
      },
      get tools() {
        return TOOLS;
      },
      get files() {
        return files;
      },
      get busy() {
        return busy;
      },
      get update() {
        return upd;
      },
      startConvert,
      go,
      dropAt,
      openWith,
      reloadRecents,
      refreshTools,
      readDefaults: async () => {
        await readDefaults();
        if (place === 'defaults') syncDefaults();
      },
    };
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      root.dataset.ready = '1';
      be.startupDone({ navigationStartToScript_ms: t0, painted_ms: performance.now() });
    })
  );
})();
