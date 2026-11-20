// DotAny Slides: a presentation inside the family's window. The top bar, file menu, settings, save card, close prompt,
// notice row, empty window and drop sheet are shared/web's; the slides, the dock, the panels' insides, editing, the show
// and the presenter's view are here. Reading and writing the file is deck.js, paint.js, chart.js and edit.js.
import { createShell } from '@dotany/web/shell.js';
import { el, root, reduce, icon, sound, EASE, store, app as family } from '@dotany/web/shared.js';
import { T, size as sizeText, langInfo } from '@dotany/web/i18n.js';
import { LUCIDE } from './icons.js';
import * as own from './backend.js';
import { SystemFilm } from './system-film.js';
import { readDeck, Encrypted, fontInstalled, kids, kid, str } from './deck.js';
import {
  NS,
  NAMES,
  setNotes as E_setNotes,
  setBox as E_setBox,
  writeBody as E_writeBody,
  setAlign as E_setAlign,
  setBullets as E_setBullets,
  setFill as E_setFill,
  setLine as E_setLine,
  addTextBox as E_addTextBox,
  addShape as E_addShape,
  addPicture as E_addPicture,
  addMedia as E_addMedia,
  addRel as E_addRel,
  cloneShape as E_clone,
  zMove as E_zMove,
  setTransition as E_setTransition,
  EFFECTS,
  effectKey as E_effectKey,
  addEffect as E_addEffect,
  clearEffects as E_clearEffects,
  moveClick as E_moveClick,
  addSlide as E_addSlide,
  duplicateSlide as E_duplicateSlide,
  removeSlide as E_removeSlide,
  moveSlides as E_moveSlides,
  setHidden as E_setHidden,
  saveDeck,
  mainTypeOf,
  setMainType,
  dropMacros,
} from './edit.js';
import { paintSlide } from './paint.js';
import { exportPng, pngPaths } from './picture.js';
import { modKey, platform, os, yRedoes } from '@dotany/web/keys.js';
import { on as hear } from '@dotany/web/backend.js';
import { presenterWindow } from './presenter.js';

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
// a PDF and pictures are copies to send: the window stays on the presentation
const FORMATS = {
  pdf: { ext: '.pdf', label: 'PDF', export: true },
  png: { ext: '.png', label: 'PNG', export: true },
  pptx: { ext: '.pptx', label: 'PPTX' },
};
// what a PowerPoint-family file keeps its own kind as; the macro-enabled kinds (pptm, ppsm, potm) are only read and
// become a pptx
const KIND = {
  pptx: ['.pptx', 'PPTX'],
  ppsx: ['.ppsx', 'PPSX'],
  potx: ['.potx', 'POTX'],
  pptm: ['.pptx', 'PPTX'],
  ppsm: ['.pptx', 'PPTX'],
  potm: ['.pptx', 'PPTX'],
};
const READ_ONLY = ['pptm', 'ppsm', 'potm'];

// words with a number in them, said in the window's language
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const MOVE = { duration: 240, easing: EASE },
  GROW = { duration: 420, easing: EASE },
  QUICK = { duration: 140, easing: EASE };
const money = (b) => sizeText(b, { zero: true, gb: true });
const NS_R = NS.r;

// ---------- what's open ----------
let opened = false,
  fileName = '',
  fileSize = 0,
  deck = null,
  pages = [],
  nowAt = 0,
  pagesMode = false,
  locked = false,
  macros = false,
  kindOf = 'pptx';
// a deck that has been read but not yet shown: the shell shows it once Rust has adopted the file
let pending = null;

// the Mac with two screens: this window is the presenter's view of another window's show (presenter.js), and nothing else
if (platform === 'mac' && window.__DOTANY__ && window.__DOTANY__.presenterOf) {
  presenterWindow(LUCIDE);
  await new Promise(() => {});
}

const shell = createShell({
  name: 'DotAny Slides',
  icons: LUCIDE,
  words: WORDS,
  notices: () => import('./notices.generated.js'),
  dropTitle: '拖拽演示文稿至此',
  fileKind: '演示文稿',
  soundNote: '触发按钮操作时播放提示音。',
  licences: [['fflate', '解压缩', 'MIT']],
  formats: FORMATS,
  // a copy to send is what people came for, unless there's something unsaved; a file only read becomes a pptx
  // each time the card opens: all pages, as they are, hidden ones left out
  formatOf: () => {
    sizeKey = 'same';
    rangeKey = 'all';
    withHidden = false;
    return locked || dirty2() ? 'pptx' : 'pdf';
  },
  cantOpen: { damaged: '文件已损坏，或并非有效的演示文稿。', encrypted: '暂不支持打开受密码保护的演示文稿。' },
  failWords: { picture: '导出幻灯片图像失败。' },
  open: openDeck,
  show() {
    if (pending) {
      const p = pending;
      pending = null;
      holdDeck(p.deck, p.r);
    }
  },
  clear: goEmpty,
  dirty: () => dirty2(),
  discard() {
    savedAt = hAt;
  },
  estimate: (fmt) => estimate(fmt),
  saveRows: (box, fmt, again) => drawCard(box, fmt, again),
  saveNote: (fmt) => (fmt === 'pptx' && locked ? '可编辑的新文档，原文件不变。' : ''),
  save: (path, fmt) => saveTo(path, fmt),
  saved: (r, fmt) => afterSave(r, fmt),
  keys: (e) => keyDown(e),
  // from one panel to the next the glass morphs
  sideMorph: true,
});
const { stage, head } = shell;
// how tall the top is now: the bar, and the notice row when there is one. The slide sits below it (Recorder does the same)
let chromeH = 53;
new ResizeObserver(() => {
  chromeH = shell.chrome.offsetHeight;
  root.style.setProperty('--chrome-now', chromeH + 'px');
  if (opened && !pagesMode) layout();
}).observe(shell.chrome);
const say = (text) => shell.say(T(text));
// a film or sound playing in a slide: a button's tik doesn't land on top of it
family.quiet = () => !!(playing && !playing.paused());

// ---------- the window: the slides and dock inside the family's stage ----------
const list = el('div', 'pages');
list.id = 'pages';
stage.appendChild(list);
const pagesBtn = shell.headButton('pagesBtn', 'grid', '幻灯片导航');
pagesBtn.setAttribute('aria-pressed', 'false');
const animSide = shell.sidePanel('anim', 'anim', '动画'),
  notesSide = shell.sidePanel('notes', 'notes', '演讲者备注'),
  infoSide = shell.sidePanel('info', 'info', '信息');
const SIDES = { anim: animSide, notes: notesSide, info: infoSide };

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
const pageNo = el('button', 'tb pct page-no dyn', '1 / 1');
pageNo.type = 'button';
pageNo.id = 'pageNo';
pageNo.setAttribute('aria-label', '跳转至幻灯片');
pageNo.dataset.tip = '跳转至幻灯片';
pageNo.addEventListener('mousedown', (e) => e.preventDefault());
const pageBox = el('span', 'fi page-in'),
  pageIn = document.createElement('input');
pageIn.type = 'text';
pageIn.id = 'pageIn';
pageIn.inputMode = 'numeric';
pageIn.setAttribute('aria-label', '跳转至幻灯片');
pageBox.appendChild(pageIn);
pageBox.hidden = true;
const playBtn = el('button', 'tb has-lab');
playBtn.type = 'button';
playBtn.id = 'playBtn';
playBtn.setAttribute('aria-label', '开始放映');
playBtn.dataset.tip = '从当前页放映 · {Shift+F5}';
playBtn.innerHTML = icon('play') + '<span>开始放映</span>';
playBtn.addEventListener('mousedown', (e) => e.preventDefault());
playBtn.addEventListener('click', () => {
  sound();
  startShow(nowAt);
});
const undoBtn = barBtn('undo', 'undo', '撤销 · {Mod+Z}', () => undo()),
  redoBtn = barBtn('redo', 'redo', '重做 · {Mod+Y}', () => redo());
const addBtn = (id, n, label) => {
  const q = el('button', 'tb add-lab');
  q.type = 'button';
  q.id = id;
  q.setAttribute('aria-label', label);
  q.dataset.tip = label;
  q.innerHTML = icon(n);
  q.addEventListener('mousedown', (e) => e.preventDefault());
  q.addEventListener('click', () => sound());
  return q;
};
const addTextBtn = addBtn('addText', 'type', '文本框'),
  addPicBtn = addBtn('addPic', 'image', '图像'),
  addShapeBtn = addBtn('addShape', 'shapes', '形状'),
  addMediaBtn = addBtn('addMedia', 'film', '音视频');
const grp = (items, c) => {
  const q = el('div', 'grp ' + (c || ''));
  q.append(...items);
  return q;
};
const pickInfo = el('span', 'pick-info dyn'),
  pagesAll = el('button', 'btn soft', '全选'),
  pagesDone = el('button', 'btn primary', '完成');
pagesAll.type = pagesDone.type = 'button';
pagesAll.id = 'pagesAll';
pagesDone.id = 'pagesDone';
[pagesAll, pagesDone].forEach((q) => q.addEventListener('mousedown', (e) => e.preventDefault()));
pagesAll.addEventListener('click', () => {
  sound();
  picked = picked.size === pages.length ? new Set() : new Set(pages);
  syncPicked();
});
pagesDone.addEventListener('click', () => {
  sound();
  leavePages();
});
const pagesHead = el('div', 'pages-head pages-only');
pagesHead.append(pickInfo, el('i', 'pages-gap'), pagesAll, pagesDone);
const named = (q, text) => {
  q.classList.add('has-lab');
  const s = el('span', 'stack-lab', text);
  q.appendChild(s);
  return s;
};
const dupPages = barBtn('pageDup', 'copy', '创建副本', () => duplicatePicked()),
  hidePages = barBtn('pageHide', 'hidden', '隐藏此页', () => hidePicked()),
  delPages = barBtn('pageDelete', 'trash', '删除', () => deletePicked());
named(dupPages, '创建副本');
const hideLab = named(hidePages, '隐藏此页');
hideLab.classList.add('dyn');
named(delPages, '删除');
const pill = el('div', 'pill'),
  pillRow = el('div', 'pill-row');
pillRow.append(
  grp([undoBtn, redoBtn], 'read-only'),
  el('i', 'sep read-only'),
  pageNo,
  pageBox,
  el('i', 'sep read-only'),
  grp([addTextBtn, addPicBtn, addShapeBtn, addMediaBtn], 'read-only'),
  el('i', 'sep read-only'),
  playBtn,
  grp([dupPages, hidePages, delPages], 'pages-only')
);
pill.append(pagesHead, pillRow);
const addTile = el('div', 'add-tile'),
  addBlank = el('button', 'add-blank');
addBlank.type = 'button';
addBlank.id = 'pageAdd';
addBlank.innerHTML = icon('plus') + '<span class="dyn">新建幻灯片</span>';
addBlank.addEventListener('mousedown', (e) => e.preventDefault());
addBlank.addEventListener('click', () => {
  sound();
  addPage(pages.length);
});
addTile.appendChild(addBlank);
const navPrev = barBtn('prevPage', 'prev', '上一页', () => goPage(nowAt - 1)),
  navNext = barBtn('nextPage', 'next', '下一页', () => goPage(nowAt + 1));
navPrev.classList.add('pic-nav');
navNext.classList.add('pic-nav');
const dockRow = el('div', 'dock-row');
dockRow.append(navPrev, pill, navNext);
shell.place(dockRow);
shell.translates(dockRow);

// ---------- pages: the same slide, drawn once, at whatever size it's needed ----------
const panelKind = () =>
  animSide.classList.contains('on')
    ? 'anim'
    : notesSide.classList.contains('on')
      ? 'notes'
      : infoSide.classList.contains('on')
        ? 'info'
        : '';
const panelOn = () => !!panelKind();
const panelW = () => (panelOn() ? 288 : 0);
const room = () => {
  const w = stage.clientWidth - 48 - panelW(),
    h = stage.clientHeight - chromeH - 16 - 96;
  return { w: Math.max(80, w), h: Math.max(60, h) };
};
const readK = () => {
  const r = room();
  return Math.min(r.w / deck.w, r.h / deck.h);
};
function gridK() {
  const r = stage.clientWidth - 64 - panelW(),
    min = deck.w >= deck.h * 1.5 ? 248 : 208,
    n = Math.max(2, Math.floor((Math.min(r, 1180) + 28) / (min + 28))),
    w = (Math.min(r, 1180) - 28 * (n - 1)) / n;
  return Math.min(w, 320) / deck.w;
}
function sizePage(p, k) {
  p.el.style.width = deck.w * k + 'px';
  p.el.style.height = deck.h * k + 'px';
  p.inner.style.transform = `scale(${k})`;
}
function ensure(p) {
  if (p.painted) return;
  p.painted = true;
  p.inner.textContent = '';
  p.inner.appendChild(paintSlide(deck, p.i));
}
function makePage(i) {
  const e = el('div', 'pg'),
    inner = el('div', 'pg-in'),
    no = el('span', 'pg-no'),
    eye = el('i', 'pg-eye'),
    noText = el('span', '', String(i + 1));
  eye.innerHTML = icon('hidden');
  e.dataset.i = i;
  const hid = deck.slides[i].hidden;
  e.classList.toggle('hid', hid);
  eye.hidden = !hid;
  no.append(eye, noText);
  e.append(inner, no);
  return { i, el: e, inner, no, eye, noText, painted: false, part: deck.slides[i].part };
}
const near = new IntersectionObserver(
  (list_) => {
    for (const x of list_)
      if (x.isIntersecting) {
        const p = pages[+x.target.dataset.i];
        if (p) ensure(p);
      }
  },
  { root: stage, rootMargin: '500px 0px' }
);
function layout() {
  if (!deck) return;
  const k = pagesMode ? gridK() : readK();
  pages.forEach((p, i) => {
    sizePage(p, k);
    p.el.classList.toggle('cur', i === nowAt);
  });
  if (pagesMode) {
    root.style.setProperty('--tile-w', deck.w * k);
    root.style.setProperty('--tile-h', deck.h * k);
  }
  if (!pagesMode) ensure(pages[nowAt]);
  else
    pages.forEach((p) => {
      if (p.painted || !p.el.isConnected) return;
      const r = p.el.getBoundingClientRect(),
        s = stage.getBoundingClientRect();
      if (r.top < s.bottom + 500 && r.bottom > s.top - 500) ensure(p);
    });
  if (selEl) placeFrame();
  syncPads();
}
function syncPads() {
  root.style.setProperty('--pad-r', panelW() + 'px');
  root.style.setProperty('--gut', pagesMode ? stage.offsetWidth - stage.clientWidth + 'px' : '0px');
}
function syncPageNo() {
  const n = pages.length;
  pageNo.textContent = `${nowAt + 1} / ${n}`;
  navPrev.disabled = nowAt <= 0;
  navNext.disabled = nowAt >= n - 1;
  pages.forEach((p, i) => p.el.classList.toggle('cur', i === nowAt));
}
function goPage(i, quiet) {
  i = clamp(i, 0, pages.length - 1);
  if (i === nowAt && !quiet) return;
  flushNotes();
  if (editingText) finishText();
  clearSelection();
  stopMedia();
  stopPreview();
  if (pagesMode) {
    nowAt = i;
    syncPageNo();
    return;
  }
  const was = pages[nowAt],
    to = pages[i],
    stay = i !== nowAt && !reduce ? was : null;
  ensure(to);
  nowAt = i;
  if (stay) {
    const a = stay.el.getBoundingClientRect(),
      s = stage.getBoundingClientRect();
    stay.el.classList.add('leaving');
    stay.el.style.left = a.left - s.left + 'px';
    stay.el.style.top = a.top - s.top + 'px';
  }
  syncPageNo();
  if (stay) {
    const f = to.el.animate([{ opacity: 0 }, { opacity: 1 }], QUICK);
    f.onfinish = f.oncancel = () => {
      stay.el.classList.remove('leaving');
      stay.el.style.left = stay.el.style.top = '';
    };
  }
  if (panelOn()) renderPanel();
  [i - 1, i + 1].forEach((j) => {
    const p0 = pages[j];
    if (p0)
      (window.requestIdleCallback || setTimeout)(() => {
        if (pages[j] === p0) ensure(p0);
      });
  });
}
// the page number becomes a box you can type in
pageNo.addEventListener('click', () => {
  sound();
  pageNo.hidden = true;
  pageBox.hidden = false;
  pageIn.value = String(nowAt + 1);
  pageIn.focus();
  pageIn.select();
  if (!reduce) pageBox.animate([{ opacity: 0 }, { opacity: 1 }], QUICK);
});
const closeJump = () => {
  pageBox.hidden = true;
  pageNo.hidden = false;
  if (!reduce) pageNo.animate([{ opacity: 0 }, { opacity: 1 }], QUICK);
};
pageIn.addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    const n = parseInt(pageIn.value, 10);
    if (n) {
      if (pagesMode) leavePages(n - 1);
      else goPage(n - 1);
    }
    closeJump();
    stage.focus({ preventScroll: true });
  } else if (e.key === 'Escape') {
    closeJump();
    stage.focus({ preventScroll: true });
  }
});
pageIn.addEventListener('blur', closeJump);
// the wheel turns one page at a time, however hard it's flicked
let wheelAt = 0;
stage.addEventListener(
  'wheel',
  (e) => {
    if (!opened || pagesMode || editingText) return;
    e.preventDefault();
    const t = performance.now();
    if (t - wheelAt < 240 || Math.abs(e.deltaY) < 8) return;
    wheelAt = t;
    goPage(nowAt + Math.sign(e.deltaY));
  },
  { passive: false }
);
list.addEventListener('dblclick', (e) => {
  if (!pagesMode) return;
  const g = e.target.closest('.pg');
  if (g) {
    sound();
    leavePages(+g.dataset.i);
  }
});
addEventListener('resize', () => {
  if (!opened) return;
  layout();
  if (showing) layoutShow();
});

// ---------- the pages view: the same paper draws back into a grid ----------
// the dock grows and shrinks to fit what it holds
function dockGrow(w0, h0, timing = GROW) {
  if (reduce) return;
  pill.getAnimations().forEach((q) => q.cancel());
  const w1 = pill.offsetWidth,
    h1 = pill.offsetHeight;
  pill.style.overflow = 'hidden';
  const f = pill.animate(
    [
      { width: w0 + 'px', height: h0 + 'px' },
      { width: w1 + 'px', height: h1 + 'px' },
    ],
    timing
  );
  f.onfinish = f.oncancel = () => {
    pill.style.overflow = '';
  };
  [...pill.querySelectorAll('.pages-head, .pill-row > *')].forEach((e) => {
    if (e.offsetParent) e.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 120, fill: 'backwards' });
  });
}
const settleFlip = () => requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove('flipping')));
function enterPages() {
  if (pagesMode || !pages.length) return;
  pagesBtn.setAttribute('aria-pressed', 'true');
  clearSelection();
  stopMedia();
  stopPreview();
  picked = new Set();
  syncPicked();
  const at = pages[nowAt],
    a = at.el.getBoundingClientRect(),
    w0 = pill.offsetWidth,
    h0 = pill.offsetHeight;
  root.classList.add('flipping');
  pagesMode = true;
  root.classList.add('pages-mode');
  layout();
  dockGrow(w0, h0);
  pages.forEach((p) => near.observe(p.el));
  const r = at.el.offsetTop - stage.clientHeight / 2 + at.el.offsetHeight / 2;
  stage.scrollTop = Math.max(0, r);
  if (!reduce) {
    const b = at.el.getBoundingClientRect(),
      box = stage.getBoundingClientRect();
    at.el.animate(
      [
        {
          transformOrigin: '0 0',
          transform: `translate(${a.left - b.left}px, ${a.top - b.top}px) scale(${a.width / b.width})`,
        },
        { transformOrigin: '0 0', transform: 'none' },
      ],
      GROW
    );
    pages.forEach((p) => {
      if (p === at) return;
      const q = p.el.getBoundingClientRect();
      if (q.bottom > box.top && q.top < box.bottom)
        p.el.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 120, fill: 'backwards' });
    });
  }
  settleFlip();
}
function leavePages(toI) {
  if (!pagesMode) return;
  pagesBtn.setAttribute('aria-pressed', 'false');
  const pickedFirst = pages.findIndex((p) => picked.has(p)),
    w0 = pill.offsetWidth,
    h0 = pill.offsetHeight;
  nowAt = clamp(toI != null ? toI : pickedFirst >= 0 ? pickedFirst : nowAt, 0, pages.length - 1);
  picked = new Set();
  syncPicked();
  syncPageNo();
  const at = pages[nowAt],
    box = stage.getBoundingClientRect(),
    first = at.el.getBoundingClientRect();
  const ghosts = reduce
    ? []
    : pages
        .filter((p) => {
          if (p === at) return false;
          const q = p.el.getBoundingClientRect();
          return q.bottom > box.top && q.top < box.bottom;
        })
        .map((p) => {
          const q = p.el.getBoundingClientRect(),
            g = p.el.cloneNode(true);
          g.classList.add('pg-ghost');
          g.classList.remove('cur');
          g.style.cssText += `;left:${q.left}px;top:${q.top}px;width:${q.width}px;height:${q.height}px;margin:0`;
          return g;
        });
  root.classList.add('flipping');
  pagesMode = false;
  root.classList.remove('pages-mode');
  stage.scrollTop = 0;
  layout();
  renderPanel();
  dockGrow(w0, h0, MOVE);
  ghosts.forEach((g) => {
    shell.app.insertBefore(g, stage.nextSibling);
    const f = g.animate([{ opacity: 1 }, { opacity: 0 }], MOVE);
    f.onfinish = f.oncancel = () => g.remove();
  });
  if (!reduce) {
    const b = at.el.getBoundingClientRect();
    at.el.animate(
      [
        {
          transformOrigin: '0 0',
          transform: `translate(${first.left - b.left}px, ${first.top - b.top}px) scale(${first.width / b.width})`,
        },
        { transformOrigin: '0 0', transform: 'none' },
      ],
      MOVE
    );
  }
  settleFlip();
}
pagesBtn.addEventListener('click', () => {
  sound();
  if (pagesMode) leavePages();
  else enterPages();
});

// ---------- the panels: the family's side panels, one per button ----------
function refit(change) {
  if (!opened || reduce || pagesMode) {
    change();
    return;
  }
  const p = pages[nowAt],
    a = p.el.getBoundingClientRect();
  change();
  const b = p.el.getBoundingClientRect();
  if (Math.abs(a.width - b.width) < 0.5 && Math.abs(a.left - b.left) < 0.5) return;
  p.el.animate(
    [
      {
        transformOrigin: '0 0',
        transform: `translate(${a.left - b.left}px, ${a.top - b.top}px) scale(${a.width / b.width})`,
      },
      { transformOrigin: '0 0', transform: 'none' },
    ],
    GROW
  );
}
let panelShown = '';
// the shell opens and closes the panels; the slide makes room for them, and the one that opens is filled
function renderPanel() {
  const kind = opened ? panelKind() : '';
  if (!kind) {
    flushNotes();
    if (panelShown) {
      panelShown = '';
      ax = null;
      notesBox = null;
      refit(() => layout());
    }
    return;
  }
  if (kind === panelShown) {
    if (kind === 'anim') syncAnim();
    else if (kind === 'notes') syncNotes();
    else fillPanel(kind);
    return;
  }
  flushNotes();
  const had = !!panelShown;
  fillPanel(kind);
  panelShown = kind;
  if (!had) refit(() => layout());
  if (kind === 'anim') syncAnim();
}
const watchPanels = new MutationObserver(() => {
  if (opened && !showing && panelKind() !== panelShown) renderPanel();
});
Object.values(SIDES).forEach((s) => watchPanels.observe(s, { attributes: true, attributeFilter: ['class'] }));
function fillPanel(kind) {
  const side = SIDES[kind];
  Object.values(SIDES).forEach((s) => {
    if (s !== side) s.textContent = '';
  });
  side.textContent = '';
  ax = null;
  notesBox = null;
  const body = el('div', 'side-body');
  if (kind === 'notes') {
    const top = el('div', 'notes-head');
    notesPage = el('span', 'notes-page dyn');
    top.append(el('h3', 'dyn', T('演讲者备注')), notesPage);
    side.append(top, body);
    buildNotes(body);
    syncNotes();
    return;
  }
  side.append(el('h3', 'dyn', T(kind === 'anim' ? '动画' : '信息')), body);
  if (kind === 'anim') {
    axTab = selEl ? 'elem' : 'page';
    buildAnim(body);
    syncAnim();
  } else panelInfo(body);
}
// the words behind this page, written straight into the file
let notesBox = null,
  notesFor = null,
  notesTimer = 0,
  notesPage = null,
  notesMeta = null;
// how long it takes to say: about four characters of Chinese or two and a half English words a second
function spoken(text) {
  const cjk = (text.match(/[㐀-鿿]/g) || []).length,
    words = (text.replace(/[㐀-鿿]/g, ' ').match(/[A-Za-z0-9'’-]+/g) || []).length,
    count = cjk + words,
    secs = Math.max(1, Math.round(cjk / 4 + words / 2.5));
  if (!count) return '';
  const time = secs < 60 ? T('{n} 秒', { n: secs }) : T('{n} 分钟', { n: Math.round(secs / 60) });
  return T('{count} 字 · 约 {time}', { count, time });
}
function syncMeta() {
  if (!notesMeta) return;
  const t = spoken(notesBox.value);
  if (notesMeta.textContent !== t) notesMeta.textContent = t;
  notesMeta.classList.toggle('on', !!t);
}
function buildNotes(body) {
  notesMeta = el('p', 'notes-meta dyn');
  notesBox = document.createElement('textarea');
  notesBox.id = 'notesIn';
  notesBox.className = 'notes-in';
  notesBox.spellcheck = false;
  notesBox.rows = 1;
  notesBox.readOnly = locked;
  notesBox.setAttribute('aria-label', T('演讲者备注'));
  notesBox.placeholder = locked ? '' : T('在此添加演讲提纲或备忘要点…');
  notesBox.addEventListener('input', () => {
    fitNotes();
    syncMeta();
    clearTimeout(notesTimer);
    notesTimer = setTimeout(flushNotes, 500);
  });
  notesBox.addEventListener('blur', flushNotes);
  notesBox.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Escape') notesBox.blur();
  });
  const pad = el('div', 'notes-pad');
  pad.append(notesBox, notesMeta);
  body.appendChild(pad);
  let wide = 0;
  const box = notesBox;
  new ResizeObserver(() => {
    if (box !== notesBox) return;
    const w = box.offsetWidth;
    if (w && w !== wide) {
      wide = w;
      fitNotes();
    }
  }).observe(box);
}
function fitNotes() {
  if (!notesBox || !notesBox.offsetParent) return;
  const h0 = notesBox.offsetHeight;
  notesBox.style.overflowY = 'hidden';
  notesBox.style.height = 'auto';
  const need = notesBox.scrollHeight + 1,
    h1 = Math.max(need, 56);
  notesBox.style.height = h0 + 'px';
  void notesBox.offsetHeight;
  notesBox.style.height = h1 + 'px';
  const most = parseFloat(getComputedStyle(notesBox).maxHeight);
  if (need > most) notesBox.style.overflowY = 'auto';
}
function syncNotes() {
  if (!notesBox) return;
  const s = curSlide(),
    pg = T('第 {n} 页', { n: nowAt + 1 });
  if (notesPage && notesPage.textContent !== pg) {
    const had = notesPage.textContent;
    notesPage.textContent = pg;
    if (had && !reduce) notesPage.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  }
  if (notesFor !== s.part || document.activeElement !== notesBox) {
    const t = s.notes || '';
    if (notesBox.value !== t) {
      notesBox.value = t;
      if (!reduce && notesFor !== s.part) notesBox.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
    }
    notesFor = s.part;
    fitNotes();
  }
  syncMeta();
}
function flushNotes() {
  clearTimeout(notesTimer);
  if (locked || !notesBox || !deck || !notesFor) return;
  const s = deck.slides.find((x) => x.part === notesFor);
  if (!s || (s.notes || '') === notesBox.value.replace(/\s+$/, '')) return;
  if (E_setNotes(deck, s, notesBox.value.replace(/\s+$/, ''))) commit();
}
const ratio = () => {
  const r = deck.w / deck.h;
  return Math.abs(r - 16 / 9) < 0.02
    ? '16:9'
    : Math.abs(r - 4 / 3) < 0.02
      ? '4:3'
      : Math.abs(r - 16 / 10) < 0.02
        ? '16:10'
        : Math.abs(r - 1) < 0.02
          ? '1:1'
          : r.toFixed(2) + ':1';
};
function panelInfo(body) {
  pages.forEach(ensure);
  const dl = el('dl', 'info-list'),
    row = (k, v, fromFile) => {
      if (v == null || v === '') return;
      const d = el('div'),
        dd = el('dd', 'dyn', v);
      if (fromFile) dd.dataset.file = '';
      d.append(el('dt', 'dyn', T(k)), dd);
      dl.appendChild(d);
    };
  const hid = deck.slides.filter((s) => s.hidden).length,
    anim = deck.slides.filter((s) => s.stepCount).length,
    media = Object.keys(deck.files).filter((n) => /\.(mp4|m4v|mov|wmv|avi|mpe?g|mp3|m4a|wav|wma)$/i.test(n)).length;
  row('文稿标题', deck.title, true);
  row('作者', deck.author, true);
  row('格式', (fileName.split('.').pop() || 'pptx').toUpperCase());
  row('幻灯片总数', String(pages.length));
  if (hid) row('隐藏页数', String(hid));
  row('画幅比例', ratio());
  row('文件大小', money(fileSize));
  if (anim) row('动画', T('{n} 页有动画', { n: anim }));
  if (media) row('视频与音频', String(media));
  if (deck.unshown) row('图像', T('{n} 张无法显示', { n: deck.unshown }));
  body.appendChild(dl);
  const names = [...deck.fonts].filter(Boolean).sort();
  if (names.length) {
    const box = el('div');
    box.appendChild(el('span', 'side-label dyn', T('文档字体')));
    const ul = el('div', 'font-list');
    names.forEach((n) => {
      const ok = fontInstalled(n),
        r = el('div', 'font-row' + (ok ? '' : ' miss'));
      const fn = el('span', 'font-n', n);
      fn.dataset.file = '';
      r.appendChild(fn);
      if (!ok) {
        r.dataset.tip = T('系统未安装，已使用相近字体替代渲染');
        r.appendChild(el('span', 'font-m dyn', T('未安装')));
      }
      ul.appendChild(r);
    });
    box.appendChild(ul);
    body.appendChild(box);
  }
}

// ---------- the show ----------
const show = el('div', 'show');
show.id = 'show';
const showMain = el('div', 'show-main'),
  showHold = el('div', 'show-hold'),
  veil = el('div', 'show-veil'),
  endCard = el('div', 'show-end'),
  laser = el('i', 'laser');
endCard.appendChild(el('span', '', '放映结束'));
showMain.append(showHold, veil, endCard);
const pvSide = el('div', 'pv-side'),
  pvTime = el('div', 'pv-time'),
  pvClock = el('span', 'pv-clock dyn', '00:00'),
  pvNow = el('span', 'pv-now dyn'),
  pvReset = el('button', 'btn soft', '重置计时器');
pvReset.type = 'button';
pvReset.id = 'pvReset';
pvTime.append(pvClock, pvNow, el('i', 'pages-gap'), pvReset);
const pvNextLab = el('span', 'side-label dyn', '下一页'),
  pvNext = el('div', 'pv-next'),
  pvNotesLab = el('span', 'side-label', '演讲者备注'),
  pvNotes = el('div', 'pv-notes');
pvSide.append(pvTime, pvNextLab, pvNext, pvNotesLab, pvNotes);
const hudNo = el('span', 'hud-no dyn', '1 / 1');
const hudLaser = barBtn('hudLaser', 'laser', '激光笔 · L', () => setLaser(!laserOn)),
  hudBlack = barBtn('hudBlack', 'black', '黑屏 · B', () => setVeil(veilKind === 'black' ? '' : 'black')),
  hudPv = barBtn('hudPv', 'presenter', '演讲者视图 · V', () => setPresenter(!presenter));
[hudLaser, hudBlack, hudPv].forEach((b) => b.setAttribute('aria-pressed', 'false'));
const hud = el('div', 'pill hud'),
  hudRow = el('div', 'pill-row');
hudRow.append(
  barBtn('hudPrev', 'prev', '上一页', () => back()),
  hudNo,
  barBtn('hudNext', 'next', '下一页', () => advance()),
  el('i', 'sep'),
  hudLaser,
  hudBlack,
  hudPv,
  el('i', 'sep'),
  barBtn('hudExit', 'close', os({ windows: '退出放映 · Esc', mac: '退出放映 · ⎋' }), () => endShow())
);
hud.appendChild(hudRow);
show.append(showMain, pvSide, hud, laser);
document.body.appendChild(show);
shell.translates(show);

let showing = false,
  showAt = 0,
  showStep = 0,
  ended = false,
  presenter = false,
  // the Mac with two screens: the presenter's view is a window of its own on the other screen (presenter.js)
  split = false,
  laserOn = false,
  veilKind = '',
  idleTimer = 0,
  autoTimer = 0,
  typed = '',
  startedAt = 0,
  clockTimer = 0,
  lastShowWheel = 0;
const painted = new Map();
const freshSlide = (i) => {
  let s = painted.get(i);
  if (!s) {
    s = paintSlide(deck, i);
    painted.set(i, s);
  }
  return s;
};
const visibleFrom = (i, d) => {
  let j = i;
  while (j >= 0 && j < pages.length && deck.slides[j].hidden) j += d;
  return j;
};
// which things are in view after a given click
function applySteps(i, slide, upto, animate) {
  const s = deck.slides[i],
    parts = (t) => {
      const host = slide.querySelector(`[data-sp="${t.sp}"]`);
      if (!host) return [];
      if (!t.para) return [host];
      const out = [];
      host.querySelectorAll('p').forEach((p) => {
        const n = +p.dataset.p;
        if (n >= t.para[0] && n <= t.para[1]) out.push(p);
      });
      return out;
    };
  const seen = new Set();
  for (const t of s.steps)
    for (const e of parts(t)) {
      if (!seen.has(e)) {
        seen.add(e);
        e.style.visibility = t.cls === 'entr' ? 'hidden' : '';
      }
    }
  for (const t of s.steps) {
    if (t.step > upto) continue;
    if (t.cls !== 'entr' && t.cls !== 'exit') continue;
    for (const e of parts(t)) {
      if (t.cls === 'entr') e.style.visibility = '';
      if (animate && t.step === upto) {
        const q = playEffect(t, e);
        if (t.cls === 'exit') {
          if (q)
            q.onfinish = () => {
              e.style.visibility = 'hidden';
              q.cancel();
            };
          else e.style.visibility = 'hidden';
        }
      } else if (t.cls === 'exit') e.style.visibility = 'hidden';
    }
  }
  if (animate)
    for (const t of s.steps)
      if (t.cls === 'mediacall' && t.step === upto) {
        const h = slide.querySelector(`[data-sp="${t.sp}"]`);
        if (h && h._media && !(playing && playing.host === h)) setTimeout(() => toggleMedia(h), t.wait || 0);
      }
}
const slideK = () => {
  const b = showMain.getBoundingClientRect();
  return { k: Math.min(b.width / deck.w, b.height / deck.h), w: b.width, h: b.height };
};
function hold(slide) {
  const { k, w, h } = slideK();
  slide.style.transformOrigin = '0 0';
  slide.style.transform = `translate(${(w - deck.w * k) / 2}px, ${(h - deck.h * k) / 2}px) scale(${k})`;
  slide.style.position = 'absolute';
  slide.style.left = 0;
  slide.style.top = 0;
}
function layoutShow() {
  const s = showHold.lastElementChild;
  if (s) hold(s);
  drawNext();
}
function drawNext() {
  pvNext.textContent = '';
  pvNotes.textContent = '';
  pvNotes.scrollTop = 0;
  if (!presenter) return;
  const j = visibleFrom(showAt + 1, 1),
    next = j < pages.length ? j : -1,
    box = pvNext.getBoundingClientRect();
  pvNextLab.textContent = next >= 0 ? T('下一页') : T('放映结束');
  if (next >= 0) {
    const s = paintSlide(deck, next),
      k = Math.min(box.width / deck.w, box.height / deck.h);
    s.style.transformOrigin = '0 0';
    s.style.position = 'absolute';
    s.style.transform = `scale(${k})`;
    const w = el('div', 'pv-paper');
    w.style.width = deck.w * k + 'px';
    w.style.height = deck.h * k + 'px';
    w.appendChild(s);
    pvNext.appendChild(w);
    if (!reduce) w.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  }
  if (!reduce) pvNotes.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  const t = deck.slides[showAt].notes;
  if (t)
    t.split('\n').forEach((line) => {
      const q = el('p', '', line);
      q.dataset.file = '';
      pvNotes.appendChild(q);
    });
  else pvNotes.appendChild(el('p', 'pv-empty dyn', T('当前页暂无演讲者备注')));
}
function syncHud() {
  hudNo.textContent = `${showAt + 1} / ${pages.length}`;
  tellPresenter();
}
// ---------- the Mac with two screens: what the presenter's window shows (presenter.js) ----------
const sentPics = new Set();
let telling = 0,
  tellAgain = 0;
const asData = (blob) =>
  new Promise((ok, no) => {
    const f = new FileReader();
    f.onload = () => ok(f.result);
    f.onerror = no;
    f.readAsDataURL(blob);
  });
// a slide as the presenter's window draws it: as it is now, without its film controls
const slideHtml = (s) => {
  const c = s.cloneNode(true);
  c.querySelectorAll('.x-play, .x-line, .x-player, video, audio').forEach((e) => e.remove());
  c.style.transform = '';
  return c.outerHTML;
};
function tellPresenter() {
  if (!split) return;
  // what changed in one go is sent once; again a little later, when an exit's motion has hidden what it hides
  cancelAnimationFrame(telling);
  telling = requestAnimationFrame(() => sendPresenter());
  clearTimeout(tellAgain);
  tellAgain = setTimeout(() => sendPresenter(), 700);
}
async function sendPresenter() {
  const s = showHold.lastElementChild;
  if (!split || !s) return;
  const j = visibleFrom(showAt + 1, 1),
    cur = slideHtml(s),
    next = j < pages.length ? slideHtml(freshSlide(j)) : null,
    pics = {};
  for (const u of new Set((cur + (next || '')).match(/blob:[^"'()\s&]+/g) || []))
    if (!sentPics.has(u)) {
      sentPics.add(u);
      pics[u] = await fetch(u)
        .then((r) => r.blob())
        .then(asData)
        .catch(() => '');
    }
  own.presenterSend({
    at: showAt + 1,
    of: pages.length,
    cur,
    next,
    notes: deck.slides[showAt].notes || '',
    ended,
    veil: veilKind,
    w: deck.w,
    h: deck.h,
    startedAt,
    theme: root.dataset.theme || '',
    lang: root.lang,
    sound: store.get('dotany-sound'),
    pics,
    words: {
      next: T('下一页'),
      prev: T('上一页'),
      end: T('放映结束'),
      notes: T('演讲者备注'),
      empty: T('当前页暂无演讲者备注'),
      reset: T('重置计时器'),
      black: T('黑屏 · B'),
      exit: T(os({ windows: '退出放映 · Esc', mac: '退出放映 · ⎋' })),
    },
  });
}
if (platform === 'mac')
  hear('presenter-do', (action) => {
    if (!showing) return;
    if (action === 'gone') {
      // the presenter's window went by itself: the show ends, as Esc ends it
      if (split) endShow();
      return;
    }
    if (!split) return;
    if (action === 'ready') {
      sentPics.clear();
      tellPresenter();
    } else if (action === 'next') advance();
    else if (action === 'prev') back();
    else if (action === 'black') setVeil(veilKind === 'black' ? '' : 'black');
    else if (action === 'end') endShow();
    else if (action.startsWith('key:')) showKey({ key: action.slice(4) });
    tellPresenter();
  });
function mount(i, step, how) {
  clearTimeout(autoTimer);
  const old = showHold.lastElementChild,
    slide = freshSlide(i);
  showAt = i;
  showStep = step;
  ended = false;
  endCard.classList.remove('on');
  applySteps(i, slide, step, false);
  hold(slide);
  if (old === slide) {
    syncHud();
    drawNext();
    armAuto();
    return;
  }
  [...showHold.children].forEach((c) => {
    if (c !== old) c.remove();
  });
  showHold.appendChild(slide);
  syncHud();
  drawNext();
  const done = () => {
    if (old && old.parentElement) old.remove();
  };
  stopMedia();
  const tr =
    how === 'cut' || reduce || (deck.slides[i].trans && deck.slides[i].trans.kind === 'cut')
      ? null
      : deck.slides[i].trans;
  if (old && tr) {
    // the slide already has a transform of its own: moves go on top of it
    const b = showMain.getBoundingClientRect(),
      f = transFrames(tr, b.width, b.height),
      at = slide.style.transform,
      oldAt = old.style.transform,
      wrap = (list_, base) => list_.map((x) => (x.transform ? { ...x, transform: `${x.transform} ${base}` } : x)),
      opt = { duration: tr.dur || 500, easing: EASE };
    if (tr.kind === 'zoom') slide.style.transformOrigin = '0 0';
    const inA = slide.animate(
      tr.kind === 'zoom'
        ? [
            { opacity: 0, scale: 0.86 },
            { opacity: 1, scale: 1 },
          ]
        : wrap(f.inFrames, at),
      opt
    );
    if (f.outFrames) old.animate(wrap(f.outFrames, oldAt), opt);
    inA.onfinish = inA.oncancel = done;
  } else done();
  if (deck.slides[i].steps.some((t) => t.cls === 'mediacall' && t.step === 0)) applySteps(i, slide, 0, true);
  armAuto();
}
function armAuto() {
  const s = deck.slides[showAt],
    tr = s.trans;
  if (tr && tr.advance != null && showStep >= s.stepCount) autoTimer = setTimeout(advance, tr.advance);
}
function advance() {
  if (!showing) return;
  tellPresenter();
  if (ended) {
    endShow();
    return;
  }
  if (veilKind) {
    setVeil('');
    return;
  }
  const s = deck.slides[showAt];
  if (showStep < s.stepCount) {
    showStep++;
    applySteps(showAt, showHold.lastElementChild, showStep, true);
    armAuto();
    return;
  }
  const j = visibleFrom(showAt + 1, 1);
  if (j >= pages.length) {
    clearTimeout(autoTimer);
    ended = true;
    endCard.classList.add('on');
    return;
  }
  mount(j, 0);
}
function back() {
  if (!showing) return;
  tellPresenter();
  if (ended) {
    ended = false;
    endCard.classList.remove('on');
    return;
  }
  if (veilKind) {
    setVeil('');
    return;
  }
  if (showStep > 0) {
    showStep--;
    applySteps(showAt, showHold.lastElementChild, showStep, false);
    clearTimeout(autoTimer);
    return;
  }
  const j = visibleFrom(showAt - 1, -1);
  if (j < 0) return;
  mount(j, deck.slides[j].stepCount, 'cut');
}
function setVeil(kind) {
  veilKind = kind;
  tellPresenter();
  veil.className = 'show-veil' + (kind ? ' on ' + kind : '');
  hudBlack.setAttribute('aria-pressed', String(kind === 'black'));
}
function setLaser(on) {
  laserOn = on;
  show.classList.toggle('lasering', on);
  hudLaser.setAttribute('aria-pressed', String(on));
  if (!on) laser.style.opacity = 0;
}
function setPresenter(on) {
  // the presenter's view is on the other screen: the audience never sees it
  if (split) return;
  const s = showHold.lastElementChild,
    a = s && s.getBoundingClientRect();
  presenter = on;
  show.classList.toggle('pview', on);
  hudPv.setAttribute('aria-pressed', String(on));
  wakeHud();
  layoutShow();
  if (reduce || !s) return;
  const b = s.getBoundingClientRect(),
    at = s.style.transform;
  s.animate(
    [
      {
        transform: `translate(${a.left - b.left}px, ${a.top - b.top}px) ${at.replace(/scale\([^)]*\)/, '')} scale(${a.width / deck.w})`,
      },
      { transform: at },
    ],
    GROW
  );
  if (on)
    pvSide.animate(
      [
        { opacity: 0, transform: 'translateX(12px)' },
        { opacity: 1, transform: 'none' },
      ],
      { ...GROW, delay: 60, fill: 'backwards' }
    );
}
function tick() {
  const t = Math.floor((Date.now() - startedAt) / 1000),
    m = Math.floor(t / 60),
    h = Math.floor(m / 60),
    p = (n) => String(n).padStart(2, '0');
  pvClock.textContent = h ? `${h}:${p(m % 60)}:${p(t % 60)}` : `${p(m)}:${p(t % 60)}`;
  const d = new Date();
  pvNow.textContent = `${p(d.getHours())}:${p(d.getMinutes())}`;
}
pvReset.addEventListener('click', () => {
  sound();
  startedAt = Date.now();
  tick();
});
function wakeHud() {
  show.classList.remove('idle');
  clearTimeout(idleTimer);
  if (!presenter)
    idleTimer = setTimeout(() => {
      if (!hud.matches(':hover')) show.classList.add('idle');
      else wakeHud();
    }, 2200);
}
function startShow(i) {
  if (!opened || showing) return;
  if (editingText) finishText();
  clearSelection();
  stopMedia();
  stopPreview();
  shell.closeAll();
  if (pagesMode) leavePages(i);
  const from = pages[nowAt] ? pages[nowAt].el.getBoundingClientRect() : null;
  painted.clear();
  showing = true;
  root.classList.add('showing');
  show.classList.add('on');
  show.classList.remove('pview', 'idle');
  presenter = false;
  setVeil('');
  setLaser(false);
  typed = '';
  showHold.textContent = '';
  startedAt = Date.now();
  tick();
  clearInterval(clockTimer);
  clockTimer = setInterval(tick, 1000);
  const open = () => {
    mount(i, 0, 'cut');
    const s = showHold.lastElementChild;
    if (from && !reduce) {
      const at = s.style.transform;
      s.animate(
        [{ transform: `translate(${from.left}px, ${from.top}px) scale(${from.width / deck.w})` }, { transform: at }],
        GROW
      );
      show.animate([{ backgroundColor: 'rgba(0,0,0,0)' }, { backgroundColor: 'rgba(0,0,0,1)' }], GROW);
    }
  };
  open();
  wakeHud();
  show.focus();
  // the window itself goes full screen: the page's full screen would only fill the page
  own.fullScreen(true);
  // the Mac with two screens: the show is on the other one, the presenter's view on the Mac's own, in a window of its own
  if (platform === 'mac')
    own.presenterOpen().then((two) => {
      if (!two) return;
      if (!showing) {
        own.presenterClose();
        return;
      }
      if (presenter) setPresenter(false);
      split = true;
      hudPv.style.display = 'none';
      sentPics.clear();
      tellPresenter();
    });
}
function endShow() {
  if (!showing) return;
  stopMedia();
  showing = false;
  clearTimeout(autoTimer);
  clearTimeout(idleTimer);
  clearInterval(clockTimer);
  if (split) {
    split = false;
    hudPv.style.display = '';
    own.presenterClose();
  }
  nowAt = showAt;
  syncPageNo();
  own.fullScreen(false);
  layout();
  if (panelOn()) renderPanel();
  const leave = () => {
    show.classList.remove('on', 'pview', 'lasering', 'idle');
    root.classList.remove('showing');
    showHold.textContent = '';
    painted.clear();
    stage.focus({ preventScroll: true });
  };
  const s = showHold.lastElementChild,
    to = pages[nowAt].el.getBoundingClientRect();
  if (reduce || !s) {
    leave();
    return;
  }
  show.classList.remove('pview');
  const at = s.style.transform;
  show.classList.add('closing');
  const a = s.animate(
    [{ transform: at }, { transform: `translate(${to.left}px, ${to.top}px) scale(${to.width / deck.w})` }],
    { ...MOVE, fill: 'forwards' }
  );
  show.animate([{ backgroundColor: 'rgba(0,0,0,1)' }, { backgroundColor: 'rgba(0,0,0,0)' }], {
    ...MOVE,
    fill: 'forwards',
  });
  a.onfinish = a.oncancel = () => {
    show.classList.remove('closing');
    show.getAnimations().forEach((x) => x.cancel());
    leave();
  };
}
showMain.addEventListener('click', (e) => {
  const med = e.target.closest && e.target.closest('.x-media');
  if (med) {
    toggleMedia(med);
    return;
  }
  const link = e.target.closest && e.target.closest('.x-link');
  if (link && /^https?:/i.test(link.dataset.link)) {
    own.openLink(link.dataset.link);
    return;
  }
  advance();
});
show.addEventListener('pointermove', (e) => {
  wakeHud();
  if (laserOn) {
    laser.style.opacity = 1;
    laser.style.transform = `translate(${e.clientX}px, ${e.clientY}px)`;
  }
});
show.addEventListener(
  'wheel',
  (e) => {
    e.preventDefault();
    const t = performance.now();
    if (t - lastShowWheel < 240 || Math.abs(e.deltaY) < 8) return;
    lastShowWheel = t;
    if (e.deltaY > 0) advance();
    else back();
  },
  { passive: false }
);
addEventListener('resize', () => {
  if (showing) layoutShow();
});
function showKey(e) {
  const k = e.key;
  if (k === 'Escape') {
    if (veilKind) setVeil('');
    else endShow();
    return true;
  }
  if (k >= '0' && k <= '9') {
    typed += k;
    return true;
  }
  if (k === 'Enter' && typed) {
    const n = parseInt(typed, 10);
    typed = '';
    if (n >= 1 && n <= pages.length) mount(n - 1, 0, 'cut');
    return true;
  }
  typed = '';
  if (
    k === 'ArrowRight'
    || k === 'ArrowDown'
    || k === ' '
    || k === 'PageDown'
    || k === 'Enter'
    || k === 'n'
    || k === 'N'
  ) {
    advance();
    return true;
  }
  if (k === 'ArrowLeft' || k === 'ArrowUp' || k === 'PageUp' || k === 'Backspace' || k === 'p' || k === 'P') {
    back();
    return true;
  }
  if (k === 'Home') {
    mount(visibleFrom(0, 1), 0, 'cut');
    return true;
  }
  if (k === 'End') {
    let j = pages.length - 1;
    while (j > 0 && deck.slides[j].hidden) j--;
    mount(j, deck.slides[j].stepCount, 'cut');
    return true;
  }
  if (k === 'b' || k === 'B' || k === '.') {
    setVeil(veilKind === 'black' ? '' : 'black');
    return true;
  }
  if (k === 'w' || k === 'W' || k === ',') {
    setVeil(veilKind === 'white' ? '' : 'white');
    return true;
  }
  if (k === 'l' || k === 'L') {
    setLaser(!laserOn);
    return true;
  }
  if (k === 'v' || k === 'V') {
    setPresenter(!presenter);
    return true;
  }
  return true;
}

// ---------- changing the file: every change is one step of undo ----------
let shadow = new Map(),
  history = [],
  hAt = 0,
  savedAt = 0;
const dirty2 = () => hAt !== savedAt;
function startHistory() {
  shadow = new Map();
  history = [];
  hAt = 0;
  savedAt = 0;
  for (const p of Object.keys(deck.files))
    if (
      /^ppt\/(slides|notesSlides)\/.*\.(xml|rels)$/.test(p)
      || /^ppt\/_rels\/presentation|^ppt\/presentation\.xml$|^\[Content_Types\]\.xml$/.test(p)
    )
      shadow.set(p, deck.partText(p));
  deck.touched.clear();
  deck.gone = new Set();
  syncUndo();
}
function commit() {
  const parts = new Set([...deck.touched, ...(deck.gone || [])]),
    rec = [];
  for (const p of parts) {
    const now = deck.hasPart(p) ? deck.partText(p) : null,
      was = shadow.has(p) ? shadow.get(p) : null;
    if (now !== was) {
      rec.push({ p, before: was, after: now });
      if (now == null) shadow.delete(p);
      else shadow.set(p, now);
    }
  }
  deck.gone = new Set();
  if (!rec.length) {
    syncPages();
    return;
  }
  history.length = hAt;
  history.push(rec);
  hAt = history.length;
  syncPages(rec.map((r) => r.p));
  syncUndo();
  shell.changed();
}
function replay(step, back_) {
  const parts = [];
  for (const r of step) {
    const text = back_ ? r.before : r.after;
    if (text == null) deck.removePart(r.p);
    else deck.setPart(r.p, text);
    if (text == null) shadow.delete(r.p);
    else shadow.set(r.p, text);
    parts.push(r.p);
  }
  // an undo means the live XML nodes on screen have gone
  clearSelection();
  syncPages(parts);
  syncUndo();
  shell.changed();
}
function undo() {
  if (locked) return;
  if (editingText) finishText();
  if (hAt <= 0) return;
  hAt--;
  replay(history[hAt], true);
}
function redo() {
  if (locked) return;
  if (editingText) finishText();
  if (hAt >= history.length) return;
  replay(history[hAt], false);
  hAt++;
  syncUndo();
  shell.changed();
}
function syncUndo() {
  undoBtn.disabled = locked || hAt <= 0;
  redoBtn.disabled = locked || hAt >= history.length;
}
// the file's list of pages may have moved underneath: keep each page's paper where possible
function syncPages(changed = []) {
  const wasPart = pages[nowAt] && pages[nowAt].part;
  deck.relist();
  const byPart = new Map(pages.map((p) => [p.part, p])),
    set = new Set(changed),
    next = deck.slides.map((s, i) => {
      let p = byPart.get(s.part);
      if (!p) {
        p = makePage(i);
        p.part = s.part;
        near.observe(p.el);
      }
      return p;
    });
  pages.forEach((p) => {
    if (!next.includes(p)) {
      near.unobserve(p.el);
      p.el.remove();
    }
  });
  pages = next;
  pages.forEach((p, i) => {
    p.i = i;
    p.el.dataset.i = i;
    if (p.noText.textContent !== String(i + 1)) {
      p.noText.textContent = String(i + 1);
      if (!reduce && pagesMode) p.noText.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
    }
    const h = deck.slides[i].hidden;
    p.el.classList.toggle('hid', h);
    p.eye.hidden = !h;
    if (list.children[i] !== p.el) list.insertBefore(p.el, list.children[i] || addTile);
  });
  if (addTile.parentElement !== list || list.lastElementChild !== addTile) list.appendChild(addTile);
  const at = pages.findIndex((p) => p.part === wasPart);
  if (at >= 0) nowAt = at;
  else nowAt = clamp(nowAt, 0, pages.length - 1);
  for (const p of pages)
    if (p.painted && (set.has(p.part) || set.has(deck.relsPath(p.part)))) {
      if (p === pages[nowAt] && !pagesMode && !reduce) repaintGently(p);
      else {
        p.inner.textContent = '';
        p.painted = false;
      }
    }
  // slide numbers on later pages are drawn from the order
  layout();
  syncPageNo();
  syncPicked();
  if (selId) reselect();
  if (panelOn() && !dragState) renderPanel();
}
function repaintGently(p) {
  const old = p.inner.lastElementChild;
  if (!old) {
    p.painted = false;
    return;
  }
  const wasTop = new Map([...old.querySelectorAll(':scope > .x-top')].map((e) => [e.dataset.sp, e])),
    next = paintSlide(deck, p.i);
  p.inner.textContent = '';
  p.inner.appendChild(next);
  const seen = new Set();
  for (const e of next.querySelectorAll(':scope > .x-top')) {
    const o = wasTop.get(e.dataset.sp);
    seen.add(e.dataset.sp);
    if (!o) {
      e.animate(
        [
          { opacity: 0, scale: 0.96 },
          { opacity: 1, scale: 1 },
        ],
        MOVE
      );
      continue;
    }
    const a = o._box,
      b = e._box;
    if (
      a
      && b
      && (Math.abs(a.x - b.x) > 0.5
        || Math.abs(a.y - b.y) > 0.5
        || Math.abs(a.w - b.w) > 0.5
        || Math.abs(a.h - b.h) > 0.5)
      && !p.justDragged
    ) {
      e.animate([{ translate: `${a.x - b.x}px ${a.y - b.y}px` }, { translate: '0px 0px' }], MOVE);
      continue;
    }
    if (o.innerHTML !== e.innerHTML && !p.justDragged) {
      const g = o.cloneNode(true);
      g.classList.remove('x-top');
      g.removeAttribute('data-sp');
      g.style.pointerEvents = 'none';
      e.after(g);
      e.animate([{ opacity: 0 }, { opacity: 1 }], QUICK);
      const f = g.animate([{ opacity: 1 }, { opacity: 0 }], QUICK);
      f.onfinish = f.oncancel = () => g.remove();
    }
  }
  for (const [sp, o] of wasTop)
    if (!seen.has(sp)) {
      const g = o.cloneNode(true);
      g.classList.remove('x-top');
      g.removeAttribute('data-sp');
      g.style.pointerEvents = 'none';
      next.appendChild(g);
      const f = g.animate(
        [
          { opacity: 1, scale: 1 },
          { opacity: 0, scale: 0.96 },
        ],
        MOVE
      );
      f.onfinish = f.oncancel = () => g.remove();
    }
  p.justDragged = false;
}
const curSlide = () => deck.slides[nowAt];
const curK = () => pages[nowAt].el.offsetWidth / deck.w;

// ---------- picking a thing on the slide ----------
const frame = el('div', 'ed-frame');
frame.id = 'edFrame';
const HANDLES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w', 'rot'];
HANDLES.forEach((h) => {
  const i = el('i', 'ed-h ed-' + h);
  i.dataset.h = h;
  frame.appendChild(i);
});
const guideX = el('i', 'ed-guide ed-gx'),
  guideY = el('i', 'ed-guide ed-gy');
let selId = null,
  selEl = null,
  editingText = null,
  dragState = null;
const topAt = (id) => {
  const p = pages[nowAt];
  return p && p.inner.querySelector(`.x-top[data-sp="${id}"]`);
};
function placeFrame() {
  if (!selEl) return;
  const p = pages[nowAt],
    k = curK(),
    b = selEl._box;
  if (frame.parentElement !== p.el) p.el.appendChild(frame);
  frame.style.left = b.x * k + 'px';
  frame.style.top = b.y * k + 'px';
  frame.style.width = Math.max(b.w * k, 1) + 'px';
  frame.style.height = Math.max(b.h * k, 1) + 'px';
  frame.style.transform = b.rot ? `rotate(${b.rot}deg)` : '';
  placeBar();
}
function reselect() {
  ensure(pages[nowAt]);
  frame.getAnimations().forEach((a) => a.cancel());
  const e = topAt(selId);
  if (!e || !e._box) {
    clearSelection();
    return;
  }
  selEl = e;
  placeFrame();
}
function selectEl(e) {
  if (editingText) finishText();
  const had = !!selEl,
    wasId = selId;
  selEl = e;
  selId = e.dataset.sp;
  shell.closePop();
  root.classList.add('picking');
  drawBar();
  frame.getAnimations().forEach((a) => a.cancel());
  if (had && wasId !== selId && !reduce) {
    frame.classList.add('glide');
    setTimeout(() => frame.classList.remove('glide'), 180);
  }
  placeFrame();
  if (!had && !reduce) frame.animate([{ opacity: 0 }, { opacity: 1 }], QUICK);
  if (panelKind() === 'anim') {
    if (axTab !== 'elem') setAxTab('elem');
    else renderPanel();
  }
}
function clearSelection() {
  if (editingText) finishText();
  const had = !!selEl;
  selEl = null;
  selId = null;
  if (had && !reduce && frame.parentElement) {
    const a = frame.animate([{ opacity: 1 }, { opacity: 0 }], QUICK);
    a.onfinish = () => {
      if (!selEl) frame.remove();
    };
  } else frame.remove();
  hideBar();
  root.classList.remove('picking');
  if (had && panelKind() === 'anim') renderPanel();
}

// ---------- moving, resizing, turning: the slide follows the hand, the file changes when it lets go ----------
const toSlide = (e) => {
  const r = pages[nowAt].el.getBoundingClientRect(),
    k = r.width / deck.w;
  return { x: (e.clientX - r.left) / k, y: (e.clientY - r.top) / k };
};
function snapTargets() {
  const xs = [0, deck.w / 2, deck.w],
    ys = [0, deck.h / 2, deck.h];
  pages[nowAt].inner.querySelectorAll('.x-top').forEach((t) => {
    if (t === selEl || !t._box || t._box.rot) return;
    const b = t._box;
    xs.push(b.x, b.x + b.w / 2, b.x + b.w);
    ys.push(b.y, b.y + b.h / 2, b.y + b.h);
  });
  return { xs, ys };
}
function snap(v, list_, tol) {
  let best = null,
    gap = tol;
  for (const t of list_) {
    const d = Math.abs(v - t);
    if (d < gap) {
      gap = d;
      best = t;
    }
  }
  return best;
}
function showBox(b) {
  const e = selEl;
  e._box = b;
  e.style.left = b.x + 'px';
  e.style.top = b.y + 'px';
  e.style.width = b.w + 'px';
  e.style.height = b.h + 'px';
  const old = e.style.transform.replace(/rotate\([^)]*\)/, '').trim();
  e.style.transform = (b.rot ? `rotate(${b.rot}deg) ` : '') + old;
  const g = e.querySelector(':scope > .x-g');
  if (g && e._node.localName !== 'grpSp') {
    g.setAttribute('width', Math.max(b.w, 1));
    g.setAttribute('height', Math.max(b.h, 1));
    const pd = dragState && dragState.b0;
    if (pd && pd.w && pd.h) {
      g.setAttribute('viewBox', `0 0 ${pd.w} ${pd.h}`);
      g.setAttribute('preserveAspectRatio', 'none');
    }
  }
  const pin = e.querySelector(':scope > .x-pin img');
  if (pin && dragState && dragState.b0) {
    const q = dragState.b0;
    pin.style.transform = `scale(${b.w / q.w}, ${b.h / q.h})`;
    pin.style.transformOrigin = `${-parseFloat(pin.style.left || 0)}px ${-parseFloat(pin.style.top || 0)}px`;
  }
  placeFrame();
}
function dragMove(e) {
  const d = dragState;
  if (!d) return;
  const at = toSlide(e),
    dx = at.x - d.s.x,
    dy = at.y - d.s.y;
  if (!d.moved) {
    if (Math.hypot(dx, dy) * curK() < 3) return;
    d.moved = true;
    hideBar();
    if (d.kind === 'move') d.snap = snapTargets();
  }
  const b0 = d.b0;
  let b = { ...b0 };
  if (d.kind === 'move') {
    b.x = b0.x + dx;
    b.y = b0.y + dy;
    guideX.remove();
    guideY.remove();
    if (!e.altKey) {
      const tol = 5 / curK();
      for (const [ed, f] of [
        [b.x, 0],
        [b.x + b.w / 2, 0.5],
        [b.x + b.w, 1],
      ]) {
        const s = snap(ed, d.snap.xs, tol);
        if (s != null) {
          b.x = s - b.w * f;
          guideX.style.left = s * curK() + 'px';
          pages[nowAt].el.appendChild(guideX);
          break;
        }
      }
      for (const [ed, f] of [
        [b.y, 0],
        [b.y + b.h / 2, 0.5],
        [b.y + b.h, 1],
      ]) {
        const s = snap(ed, d.snap.ys, tol);
        if (s != null) {
          b.y = s - b.h * f;
          guideY.style.top = s * curK() + 'px';
          pages[nowAt].el.appendChild(guideY);
          break;
        }
      }
    }
  } else if (d.kind === 'rot') {
    const cx = b0.x + b0.w / 2,
      cy = b0.y + b0.h / 2;
    let a = (Math.atan2(at.y - cy, at.x - cx) * 180) / Math.PI + 90;
    if (e.shiftKey) a = Math.round(a / 15) * 15;
    else {
      const n = Math.round(a / 90) * 90;
      if (Math.abs(a - n) < 3) a = n;
    }
    b.rot = (a + 360) % 360;
  } else {
    // resizing in the shape's own turned frame; corners of pictures keep their shape
    const r = ((b0.rot || 0) * Math.PI) / 180,
      c = Math.cos(-r),
      s = Math.sin(-r),
      lx = dx * c - dy * s,
      ly = dx * s + dy * c,
      h = d.kind;
    let w = b0.w,
      ht = b0.h;
    if (h.includes('e')) w = b0.w + lx;
    if (h.includes('w')) w = b0.w - lx;
    if (h.includes('s')) ht = b0.h + ly;
    if (h.includes('n')) ht = b0.h - ly;
    const keep = (e.shiftKey || selEl._node.localName === 'pic') && h.length === 2 && b0.h > 0;
    if (keep) {
      const q = b0.w / b0.h;
      if (Math.abs(w / b0.w) > Math.abs(ht / b0.h)) ht = w / q;
      else w = ht * q;
    }
    w = Math.max(w, 4);
    ht = Math.max(ht, selEl._node.localName === 'cxnSp' ? 0 : 4);
    const cr = Math.cos(r),
      sr = Math.sin(r);
    const ax_ = h.includes('w') ? b0.w - w : 0,
      ay = h.includes('n') ? b0.h - ht : 0,
      ccx = ax_ + w / 2 - b0.w / 2,
      ccy = ay + ht / 2 - b0.h / 2,
      wx = ccx * cr - ccy * sr,
      wy = ccx * sr + ccy * cr;
    b = { x: b0.x + b0.w / 2 + wx - w / 2, y: b0.y + b0.h / 2 + wy - ht / 2, w, h: ht, rot: b0.rot };
  }
  showBox(b);
}
function dragEnd() {
  const d = dragState;
  if (!d) return;
  dragState = null;
  guideX.remove();
  guideY.remove();
  if (!d.moved) {
    if (d.kind === 'move' && d.wasSel) {
      const t = d.textAt;
      if (t) startText(selEl, t, d.ev);
    }
    placeBar();
    return;
  }
  E_setBox(selEl._node, selEl._box);
  deck.touched.add(curSlide().part);
  pages[nowAt].justDragged = true;
  commit();
  placeBar();
}
addEventListener('pointermove', dragMove);
addEventListener('pointerup', dragEnd);
function startDrag(kind, e, wasSel, textAt) {
  dragState = {
    kind,
    s: toSlide(e),
    b0: { ...selEl._box },
    moved: false,
    wasSel,
    textAt,
    ev: { x: e.clientX, y: e.clientY },
  };
}
list.addEventListener('pointerdown', (e) => {
  if (pagesMode || e.button) return;
  const media = e.target.closest('.x-play');
  if (media) {
    e.preventDefault();
    toggleMedia(media.parentElement);
    return;
  }
  // a file that's only read: films still play, nothing is picked up
  if (locked) return;
  if (editingText && editingText.t.contains(e.target)) return;
  const h = e.target.closest('.ed-h');
  if (h) {
    e.preventDefault();
    startDrag(h.dataset.h === 'rot' ? 'rot' : h.dataset.h, e);
    return;
  }
  if (editingText) finishText();
  const top = e.target.closest('.x-top');
  if (!top) {
    clearSelection();
    return;
  }
  if (!top._box) return;
  e.preventDefault();
  const wasSel = selEl === top,
    cellT = e.target.closest('.x-t');
  if (!wasSel) selectEl(top);
  startDrag('move', e, wasSel, cellT);
});
list.addEventListener('dblclick', (e) => {
  if (pagesMode || locked) return;
  const t = e.target.closest('.x-t'),
    top = e.target.closest('.x-top');
  if (editingText && editingText.t === t) return;
  if (t && top) {
    if (selEl !== top) selectEl(top);
    startText(top, t, { x: e.clientX, y: e.clientY });
  }
});

// ---------- words, in place ----------
function startText(top, t, at) {
  if (editingText) finishText();
  const spans = [...t.querySelectorAll('span:not(.x-bu)')],
    xmlRuns = t._body
      ? kids(t._body, 'p').flatMap((p) => kids(p).filter((r) => r.localName === 'r' || r.localName === 'fld'))
      : [];
  if (t._hint) {
    t.parentElement.classList.remove('x-hint');
    const keep = t.querySelector('p'),
      sp = keep.querySelector('span:not(.x-bu)');
    t.querySelectorAll('p').forEach((p) => {
      if (p !== keep) p.remove();
    });
    keep.dataset.p = 0;
    if (sp) {
      sp.textContent = '​';
      [...keep.querySelectorAll('span:not(.x-bu)')].forEach((q) => {
        if (q !== sp) q.remove();
      });
    }
  } else
    spans.forEach((s, i) => {
      s.dataset.rk = i;
    });
  t.querySelectorAll('.x-bu').forEach((b) => {
    b.contentEditable = 'false';
  });
  t.contentEditable = 'true';
  t.spellcheck = false;
  root.classList.add('typing-slide');
  editingText = { t, top, runs: t._hint ? [] : xmlRuns, was: t.innerHTML, hint: !!t._hint };
  frame.classList.add('texting');
  drawBar();
  placeBar();
  t.focus({ preventScroll: true });
  const r = document.caretRangeFromPoint ? document.caretRangeFromPoint(at.x, at.y) : null,
    sel = getSelection();
  sel.removeAllRanges();
  if (r && t.contains(r.startContainer) && !editingText.hint) sel.addRange(r);
  else {
    const q = document.createRange();
    q.selectNodeContents(t);
    q.collapse(false);
    sel.addRange(q);
  }
}
function finishText() {
  const ed = editingText;
  if (!ed) return;
  editingText = null;
  const { t, top } = ed;
  t.contentEditable = 'false';
  root.classList.remove('typing-slide');
  frame.classList.remove('texting');
  getSelection().removeAllRanges();
  if (t.innerHTML === ed.was) {
    if (ed.hint) {
      pages[nowAt].painted = false;
      pages[nowAt].inner.textContent = '';
      ensure(pages[nowAt]);
      reselect();
    }
    drawBar();
    return;
  }
  // a text box that grows with its words keeps growing here
  if (t.classList.contains('x-grow') && top._box) {
    const inner =
      [...t.children].reduce((a, c) => a + c.offsetHeight, 0)
      + parseFloat(getComputedStyle(t).paddingTop)
      + parseFloat(getComputedStyle(t).paddingBottom);
    if (inner > 4) {
      top._box = { ...top._box, h: inner };
      E_setBox(top._node, top._box);
    }
  }
  E_writeBody(t._shape, t, ed.runs);
  deck.touched.add(curSlide().part);
  pages[nowAt].justDragged = true;
  commit();
  drawBar();
}
function applyToText(attrs) {
  const ed = editingText;
  if (!ed) return;
  const sel = getSelection();
  let nodes = [];
  if (sel.rangeCount && !sel.isCollapsed && ed.t.contains(sel.anchorNode)) {
    const r = sel.getRangeAt(0),
      w = document.createTreeWalker(ed.t, NodeFilter.SHOW_TEXT);
    const list_ = [];
    for (let n = w.nextNode(); n; n = w.nextNode())
      if (!n.parentElement.closest('.x-bu') && r.intersectsNode(n)) list_.push(n);
    const sc = r.startContainer,
      so = r.startOffset,
      ec = r.endContainer,
      eo = r.endOffset;
    list_.forEach((n) => {
      let a = 0,
        b = n.length;
      if (n === sc) a = so;
      if (n === ec) b = eo;
      if (a >= b) return;
      let mid = n;
      if (a > 0) mid = mid.splitText(a);
      if (b - a < mid.length) mid.splitText(b - a);
      nodes.push(mid);
    });
    nodes = nodes.map((n) => {
      const p = n.parentElement;
      if (p.tagName === 'SPAN' && p.childNodes.length === 1) return p;
      const s = p.tagName === 'SPAN' ? p.cloneNode(false) : document.createElement('span');
      if (p.tagName === 'SPAN') {
        const after = n.nextSibling ? p.cloneNode(false) : null;
        if (after) {
          while (n.nextSibling) after.appendChild(n.nextSibling);
          p.after(after);
        }
        p.after(s);
        s.appendChild(n);
      } else {
        n.before(s);
        s.appendChild(n);
      }
      return s;
    });
  } else nodes = [...ed.t.querySelectorAll('span:not(.x-bu)')];
  nodes.forEach((s) => {
    for (const k in attrs) {
      const v = attrs[k];
      if (k === 'b') {
        s.dataset.b = v;
        s.style.fontWeight = v === '1' ? 700 : 400;
      } else if (k === 'i') {
        s.dataset.i = v;
        s.style.fontStyle = v === '1' ? 'italic' : 'normal';
      } else if (k === 'u') {
        s.dataset.u = v;
        s.style.textDecoration = v === '1' ? 'underline' : 'none';
      } else if (k === 'sz') {
        s.dataset.sz = v;
        s.style.fontSize = v + 'px';
      } else if (k === 'c') {
        s.dataset.c = v;
        s.style.color = themeCss(v);
      }
    }
  });
  ed.t.focus({ preventScroll: true });
  if (nodes.length) {
    const r = document.createRange();
    r.setStartBefore(nodes[0]);
    r.setEndAfter(nodes[nodes.length - 1]);
    sel.removeAllRanges();
    sel.addRange(r);
  }
}
const firstSpan = () => {
  if (!editingText) return null;
  const sel = getSelection();
  let n = sel.anchorNode;
  if (n && editingText.t.contains(n)) {
    const s = (n.nodeType === 3 ? n.parentElement : n).closest('span');
    if (s && !s.classList.contains('x-bu')) return s;
  }
  return editingText.t.querySelector('span:not(.x-bu)');
};
function themeCss(k) {
  if (/^[0-9A-Fa-f]{6}$/.test(k)) return '#' + k;
  const m = deck.slides[0].master,
    name = (m && m.clrMap[k]) || k,
    h = (m && m.theme.colors[name]) || '000000';
  return '#' + h;
}

// ---------- the small sheet above what you've picked: the family's selection bar ----------
const bar = el('div', 'ed-bar');
bar.id = 'edBar';
shell.place(bar);
const hideBar = () => {
  bar.classList.remove('on', 'moving');
};
let barWas = 0;
function drawBar() {
  const w0 = bar.classList.contains('on') ? bar.offsetWidth : 0;
  bar.textContent = '';
  if (!selEl) {
    hideBar();
    return;
  }
  barWas = w0;
  const node = selEl._node,
    kind = node.localName,
    hasText = kind === 'sp';
  if (editingText) {
    const fsz = () => {
      const s = firstSpan();
      return s ? parseFloat(s.style.fontSize) || 18 : 18;
    };
    const bold = barBtn('edBold', 'bold', '加粗 · {Mod+B}', () => {
        const s = firstSpan(),
          on = s && (s.style.fontWeight === '700' || s.dataset.b === '1');
        applyToText({ b: on ? '0' : '1' });
      }),
      ital = barBtn('edItalic', 'italic', '斜体 · {Mod+I}', () => {
        const s = firstSpan(),
          on = s && s.style.fontStyle === 'italic';
        applyToText({ i: on ? '0' : '1' });
      }),
      und = barBtn('edUnder', 'underline', '下划线 · {Mod+U}', () => {
        const s = firstSpan(),
          on = s && /underline/.test(s.style.textDecoration);
        applyToText({ u: on ? '0' : '1' });
      });
    const szNo = el('span', 'ed-sz dyn', String(Math.round(fsz())));
    szNo.id = 'edSize';
    const bump = (d) => {
      const v = clamp(Math.round(fsz()) + d, 6, 200);
      applyToText({ sz: v });
      szNo.textContent = String(v);
    };
    bar.append(
      bold,
      ital,
      und,
      el('i', 'sep'),
      barBtn('edSmaller', 'minus', '缩小字号', () => bump(-2)),
      szNo,
      barBtn('edBigger', 'plus', '增大字号', () => bump(2)),
      el('i', 'sep'),
      colorBtn('edColor', 'textColor', '文字色彩', (k) => applyToText({ c: k })),
      el('i', 'sep')
    );
    [
      ['l', 'alignL', '左对齐'],
      ['ctr', 'alignC', '居中对齐'],
      ['r', 'alignR', '右对齐'],
    ].forEach(([a, ic, tip]) =>
      bar.appendChild(
        barBtn('edAlign' + a, ic, tip, () => {
          const ed = editingText;
          E_writeBody(ed.t._shape, ed.t, ed.runs);
          ed.was = '';
          E_setAlign(ed.t, a);
          finishText();
        })
      )
    );
    bar.appendChild(
      barBtn('edBullets', 'bullets', '项目符号', () => {
        const ed = editingText,
          on = !ed.t.querySelector('.x-bu');
        E_writeBody(ed.t._shape, ed.t, ed.runs);
        ed.was = '';
        E_setBullets(ed.t, on);
        finishText();
      })
    );
    translateBar();
    return;
  }
  if (hasText)
    bar.append(
      colorBtn(
        'edFill',
        'fill',
        '填充色彩',
        (k) => {
          E_setFill(node, k);
          deck.touched.add(curSlide().part);
          commit();
        },
        true
      ),
      colorBtn(
        'edLine',
        'line',
        '边框线条',
        (k) => {
          E_setLine(node, k);
          deck.touched.add(curSlide().part);
          commit();
        },
        true
      ),
      el('i', 'sep')
    );
  bar.append(
    barBtn('edFront', 'front', '上移一层', () => {
      if (E_zMove(curSlide(), node, 'up')) {
        deck.touched.add(curSlide().part);
        commit();
      }
    }),
    barBtn('edBack', 'backward', '下移一层', () => {
      if (E_zMove(curSlide(), node, 'down')) {
        deck.touched.add(curSlide().part);
        commit();
      }
    }),
    el('i', 'sep'),
    barBtn('edCopy', 'copy', '创建副本 · {Mod+D}', () => duplicateSel()),
    barBtn('edDelete', 'trash', os({ windows: '删除 · Delete', mac: '删除 · ⌫' }), () => deleteSel())
  );
  translateBar();
}
// the bar is drawn anew each time: its tips speak the window's language as they're made
function translateBar() {
  bar.querySelectorAll('[data-tip]').forEach((b) => {
    b.dataset.tip = T(b.dataset.tip);
    b.setAttribute('aria-label', T(b.getAttribute('aria-label')));
  });
}
function placeBar() {
  if (!selEl || (dragState && dragState.moved)) {
    hideBar();
    return;
  }
  const f = frame.getBoundingClientRect(),
    bw = bar.offsetWidth,
    bh = bar.offsetHeight,
    top = head.getBoundingClientRect().bottom + 8;
  let y = f.top - bh - 26;
  if (y < top) {
    y = f.bottom + 26;
    if (y + bh > innerHeight - 76) y = Math.max(top, Math.min(f.top + 10, innerHeight - 76 - bh));
  }
  bar.classList.toggle('moving', bar.classList.contains('on'));
  bar.style.left = clamp(f.left + f.width / 2 - bw / 2, 8, innerWidth - bw - 8 - panelW()) + 'px';
  bar.style.top = y + 'px';
  bar.classList.add('on');
  if (barWas && Math.abs(barWas - bw) > 1 && !reduce) {
    bar.style.overflow = 'hidden';
    const a = bar.animate([{ width: barWas + 'px' }, { width: bw + 'px' }], MOVE);
    a.onfinish = a.oncancel = () => {
      bar.style.overflow = '';
    };
    [...bar.children].forEach((c) =>
      c.animate([{ opacity: 0 }, { opacity: 1 }], { ...MOVE, delay: 60, fill: 'backwards' })
    );
  }
  barWas = 0;
}
// colours: the file's own, so a change of theme still follows
function colorBtn(id, ic, tip, pick, canNone) {
  const b = barBtn(id, ic, tip, () => {
    // the swatches hang below the button
    shell.openPop(
      b,
      (pop) => {
        const grid = el('div', 'swatches'),
          keys = ['bg1', 'tx1', 'bg2', 'tx2', 'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6'];
        keys.forEach((k) => {
          const s = el('button', 'swatch');
          s.type = 'button';
          s.id = 'sw-' + k;
          s.style.background = themeCss(k);
          s.setAttribute('aria-label', k);
          s.addEventListener('mousedown', (e) => e.preventDefault());
          s.addEventListener('click', () => {
            sound();
            shell.closePop();
            pick(k);
          });
          grid.appendChild(s);
        });
        pop.appendChild(grid);
        if (canNone) {
          const n = el('button', '');
          n.type = 'button';
          n.id = 'sw-none';
          n.appendChild(el('span', '', T('无效果')));
          n.addEventListener('mousedown', (e) => e.preventDefault());
          n.addEventListener('click', () => {
            sound();
            shell.closePop();
            pick(null);
          });
          pop.appendChild(n);
        }
      },
      null
    );
  });
  return b;
}
function deleteSel() {
  if (!selEl) return;
  const n = selEl._node,
    id = selId,
    s = curSlide();
  E_clearEffects(s, id);
  n.remove();
  deck.touched.add(s.part);
  clearSelection();
  commit();
}
function duplicateSel() {
  if (!selEl) return;
  const s = curSlide(),
    c = E_clone(s, selEl._node);
  deck.touched.add(s.part);
  const id = str(
    kid(
      kids(c).find((x) => /^nv/.test(x.localName)),
      'cNvPr'
    ),
    'id'
  );
  selId = id;
  commit();
  reselect();
  drawBar();
  placeBar();
  if (selEl) selectEl(selEl);
}
let clip = null;
function copySel(cut) {
  if (!selEl) return;
  const s = curSlide();
  clip = { xml: selEl._node.cloneNode(true), rels: deck.relsOf(s.part) };
  if (cut) deleteSel();
}
function pasteClip() {
  if (!clip) return;
  const s = curSlide(),
    c = E_clone(s, clip.xml);
  // pictures and sounds named by the copy have to exist on this page too
  const walk = (n) => {
    for (const a of [...n.attributes])
      if (a.namespaceURI === NS_R) {
        const r = clip.rels.get(a.value);
        if (
          (r && !r.external && !deck.relsOf(s.part).has(a.value))
          || (r && deck.relsOf(s.part).get(a.value) && deck.relsOf(s.part).get(a.value).target !== r.target)
        )
          n.setAttributeNS(NS_R, a.name, E_addRel(deck, s.part, r.type, r.target));
      }
    [...n.children].forEach(walk);
  };
  walk(c);
  deck.touched.add(s.part);
  selId = str(
    kid(
      kids(c).find((x) => /^nv/.test(x.localName)),
      'cNvPr'
    ),
    'id'
  );
  commit();
  reselect();
  if (selEl) selectEl(selEl);
}

// ---------- inserting: the system's open dialog, then the file's bytes from Rust ----------
function placeNew(n) {
  const s = curSlide();
  deck.touched.add(s.part);
  selId = str(
    kid(
      kids(n).find((x) => /^nv/.test(x.localName)),
      'cNvPr'
    ),
    'id'
  );
  commit();
  reselect();
  if (selEl) selectEl(selEl);
}
function fitBox(w, h, share = 0.6) {
  const k = Math.min((deck.w * share) / w, (deck.h * share) / h, 1),
    W = w * k,
    H = h * k;
  return { x: (deck.w - W) / 2, y: (deck.h - H) / 2, w: W, h: H };
}
const IMG_EXT = /\.(png|jpe?g|gif|bmp)$/i,
  VID_EXT = /\.(mp4|m4v|mov|wmv|avi)$/i,
  AUD_EXT = /\.(mp3|wav|m4a|wma|ogg|flac)$/i;
// what Windows plays rather than WebView2: its own formats (Media Foundation)
const BY_SYSTEM = /^(wmv|avi|wma|asf)$/i;
async function pickIn(kind) {
  const path = await own.pickInsert(kind, T(kind === 'image' ? '图像' : '音视频'));
  if (!path) return;
  const bytes = await own.readInsert(path).catch(() => null);
  if (!bytes) {
    say('无法读取该文件。');
    return;
  }
  insertBytes(path.split(/[\\/]/).pop(), new Uint8Array(bytes));
}
async function insertFile(f) {
  insertBytes(f.name, new Uint8Array(await f.arrayBuffer()));
}
async function insertBytes(name, bytes) {
  if (locked) return;
  const ext = (name.split('.').pop() || '').toLowerCase(),
    s = curSlide(),
    base = name.replace(/\.[^.]+$/, '');
  if (IMG_EXT.test(name)) {
    let im;
    try {
      im = await createImageBitmap(new Blob([bytes]));
    } catch {
      say('无法读取该文件。');
      return;
    }
    const b = fitBox(im.width * 0.75, im.height * 0.75);
    placeNew(E_addPicture(deck, s, bytes, ext === 'jpeg' ? 'jpg' : ext, b.x, b.y, b.w, b.h, base));
    return;
  }
  if (VID_EXT.test(name)) {
    const still = await posterOf(bytes, ext);
    if (!still) {
      say(macWindowsOnly(ext) ? 'Mac 不支持播放 WMV 格式。' : '当前系统环境无法解码播放此媒体文件。');
      return;
    }
    const b = fitBox(still.w * 0.75, still.h * 0.75, 0.7);
    placeNew(E_addMedia(deck, s, bytes, ext, 'video', still.png, b.x, b.y, b.w, b.h));
    return;
  }
  if (AUD_EXT.test(name)) {
    const c = document.createElement('canvas');
    c.width = c.height = 192;
    const x = c.getContext('2d');
    x.fillStyle = themeCss('accent1');
    x.beginPath();
    x.arc(96, 96, 96, 0, Math.PI * 2);
    x.fill();
    x.fillStyle = '#fff';
    x.beginPath();
    x.moveTo(54, 80);
    x.lineTo(78, 80);
    x.lineTo(108, 54);
    x.lineTo(108, 138);
    x.lineTo(78, 112);
    x.lineTo(54, 112);
    x.closePath();
    x.fill();
    x.strokeStyle = '#fff';
    x.lineWidth = 8;
    x.lineCap = 'round';
    x.beginPath();
    x.arc(112, 96, 22, -0.8, 0.8);
    x.stroke();
    x.beginPath();
    x.arc(112, 96, 40, -0.8, 0.8);
    x.stroke();
    placeNew(E_addMedia(deck, s, bytes, ext, 'audio', await canvasPng(c), deck.w * 0.46, deck.h * 0.42, 72, 72));
    return;
  }
  say('暂不支持该文件格式。');
}
// a film's first picture, for the slide to show before it plays: WebView2's own, or Windows' for what WebView2 won't play
async function posterOf(bytes, ext) {
  const c = document.createElement('canvas');
  // the Mac plays these in the page too, repackaged (AVI), or not at all (WMV, WMA)
  if (platform === 'mac' && BY_SYSTEM.test(ext)) {
    const film = await macFilm(bytes, ext);
    if (!film) return null;
    bytes = film;
    ext = 'mp4';
  }
  if (BY_SYSTEM.test(ext)) {
    const f = new SystemFilm(++mediaGen, mediaPort(bytes, ext));
    try {
      await f.ready();
    } catch {
      f.dispose();
      return null;
    }
    if (!f.width) {
      f.dispose();
      return null;
    }
    c.width = f.width;
    c.height = f.height;
    c.getContext('2d').drawImage(f.frame(), 0, 0);
    const r = { w: f.width, h: f.height, png: await canvasPng(c) };
    f.dispose();
    return r;
  }
  const u = URL.createObjectURL(new Blob([bytes], { type: platform === 'mac' ? filmType(bytes) : 'video/mp4' })),
    v = document.createElement('video');
  v.muted = true;
  v.src = u;
  v.preload = 'auto';
  try {
    await new Promise((res, rej) => {
      v.onloadeddata = res;
      v.onerror = rej;
    });
  } catch {
    URL.revokeObjectURL(u);
    return null;
  }
  c.width = v.videoWidth;
  c.height = v.videoHeight;
  c.getContext('2d').drawImage(v, 0, 0);
  URL.revokeObjectURL(u);
  return { w: v.videoWidth, h: v.videoHeight, png: await canvasPng(c) };
}
const canvasPng = (c) =>
  new Promise((res) => c.toBlob(async (b) => res(new Uint8Array(await b.arrayBuffer())), 'image/png'));
const SHAPES = [
  ['rect', '矩形'],
  ['roundRect', '圆角矩形'],
  ['ellipse', '椭圆'],
  ['triangle', '三角形'],
  ['rightArrow', '箭头'],
  ['chevron', '燕尾箭头'],
  ['star5', '星形'],
  ['line', '直线'],
];
addTextBtn.addEventListener('click', () => {
  if (locked) return;
  const s = curSlide(),
    n = E_addTextBox(s, deck.w * 0.32, deck.h * 0.42, deck.w * 0.36, 40);
  placeNew(n);
  if (selEl) {
    const t = selEl.querySelector('.x-t');
    if (t) startText(selEl, t, { x: -1, y: -1 });
  }
});
addPicBtn.addEventListener('click', () => pickIn('image'));
addMediaBtn.addEventListener('click', () => pickIn('media'));
addShapeBtn.addEventListener('click', () =>
  shell.openPop(
    addShapeBtn,
    SHAPES.map(([value, label]) => ({ value, label })),
    (k) => {
      const s = curSlide();
      placeNew(E_addShape(s, k, deck.w * 0.38, deck.h * 0.36, deck.w * 0.24, deck.h * 0.24));
    },
    { up: true }
  )
);
addEventListener('paste', async (e) => {
  if (!opened || locked || showing || editingText || /INPUT|TEXTAREA/.test(document.activeElement.tagName)) return;
  const item = [...(e.clipboardData ? e.clipboardData.items : [])].find((i) => /^image\//.test(i.type));
  if (item) {
    e.preventDefault();
    const f = item.getAsFile(),
      n = new File([f], 'image.png', { type: f.type });
    insertFile(n);
  } else if (clip) {
    e.preventDefault();
    pasteClip();
  }
});

// ---------- films and sounds: played where they sit, by whatever the system can decode ----------
// WebView2 plays what it can from the file's own bytes; WMV, AVI and WMA go to Windows' Media Foundation
let playing = null,
  mediaGen = 0;
const mediaPort = (bytes, ext) => ({
  open: (gen) => own.mediaOpen(gen, ext, bytes),
  control: (gen, action, value, id) => own.mediaControl(gen, action, value, id),
  close: (gen) => own.mediaClose(gen),
});
// the Mac: what WKWebView won't play as it is. WMV and WMA nothing on a Mac plays; an AVI is repackaged by the Rust side
// into a film it plays, kept per part of the deck for the next time it's played
const macWindowsOnly = (ext) => platform === 'mac' && /^(wmv|wma|asf)$/i.test(ext);
const macFilms = new Map();
async function macFilm(bytes, ext) {
  if (macWindowsOnly(ext)) return null;
  try {
    return await own.mediaRepack(ext, bytes);
  } catch {
    return null;
  }
}
// a QuickTime film says so in its first box; everything else here is an MP4
const filmType = (b) =>
  b.length > 12 && String.fromCharCode(b[8], b[9], b[10], b[11]) === 'qt  ' ? 'video/quicktime' : 'video/mp4';
async function playOnMac(e, part, ext) {
  let url = macFilms.get(part);
  if (!url) {
    const film = await macFilm(deck.files[part], ext);
    if (!film) {
      say(macWindowsOnly(ext) ? 'Mac 不支持播放 WMV 格式。' : '当前系统环境无法解码播放此媒体文件。');
      return;
    }
    url = URL.createObjectURL(new Blob([film], { type: filmType(film) }));
    macFilms.set(part, url);
  }
  if (!playing) toggleMedia(e, url);
}
function partOf(url) {
  for (const [part, u] of deck.urls) if (u === url) return part;
  return null;
}
function toggleMedia(e, macUrl) {
  if (playing && playing.host === e) {
    if (playing.paused()) playing.play();
    else playing.pause();
    e.classList.toggle('x-paused', playing.paused());
    return;
  }
  stopMedia();
  if (!e._media) return;
  const part = partOf(e._media.url),
    ext = (part || '').split('.').pop().toLowerCase(),
    line = el('i', 'x-line');
  if (platform === 'mac' && !macUrl && BY_SYSTEM.test(ext) && deck.files[part]) {
    playOnMac(e, part, ext);
    return;
  }
  if (BY_SYSTEM.test(ext) && deck.files[part] && !macUrl) {
    playBySystem(e, ext, deck.files[part], line);
    return;
  }
  const m = document.createElement(e._media.kind === 'audio' ? 'audio' : 'video');
  m.src = macUrl || e._media.url;
  m.className = 'x-player';
  m.playsInline = true;
  m.addEventListener('error', () => {
    say('当前系统环境无法解码播放此媒体文件。');
    stopMedia();
  });
  m.addEventListener('ended', () => stopMedia());
  m.addEventListener('timeupdate', () => {
    line.style.transform = `scaleX(${m.duration ? m.currentTime / m.duration : 0})`;
  });
  e.appendChild(m);
  e.appendChild(line);
  e.classList.add('x-on');
  playing = {
    host: e,
    el: m,
    line,
    paused: () => m.paused,
    play: () => m.play().catch(() => {}),
    pause: () => m.pause(),
    time: () => m.currentTime,
    stop() {
      m.pause();
      m.removeAttribute('src');
      m.load();
    },
  };
  if (!reduce && e._media.kind !== 'audio') m.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  m.play().catch(() => {});
}
async function playBySystem(e, ext, bytes, line) {
  const f = new SystemFilm(++mediaGen, mediaPort(bytes, ext)),
    me = {
      host: e,
      el: f.el,
      line,
      film: f,
      paused: () => f.paused,
      play: () => f.play(),
      pause: () => f.pause(),
      time: () => f.time,
      stop: () => f.dispose(),
    };
  playing = me;
  e.classList.add('x-on');
  try {
    await f.ready();
  } catch {
    if (playing === me) {
      // nothing on a Mac plays WMV
      say(
        ext === 'wmv'
          ? os({ windows: '当前系统环境无法解码播放此媒体文件。', mac: 'Mac 不支持播放 WMV 格式。' })
          : '当前系统环境无法解码播放此媒体文件。'
      );
      stopMedia();
    } else f.dispose();
    return;
  }
  if (playing !== me) {
    f.dispose();
    return;
  }
  f.el.className = 'x-player';
  if (!f.width) f.el.style.display = 'none';
  f.onended = () => {
    if (playing === me) stopMedia();
  };
  e.appendChild(f.el);
  e.appendChild(line);
  const step = () => {
    if (playing !== me) return;
    line.style.transform = `scaleX(${f.duration ? f.time / f.duration : 0})`;
    me.timer = setTimeout(step, 250);
  };
  step();
  f.play();
  if (!reduce && f.width) f.el.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
}
function stopMedia() {
  if (!playing) return;
  const { host, el: m, line } = playing,
    me = playing;
  playing = null;
  clearTimeout(me.timer);
  if (me.film) me.film.pause();
  else m.pause();
  host.classList.remove('x-on', 'x-paused');
  const gone = () => {
    me.stop();
    m.remove();
    line.remove();
  };
  if (reduce || !host.isConnected || m.tagName === 'AUDIO' || !m.isConnected) gone();
  else {
    const a = m.animate([{ opacity: 1 }, { opacity: 0 }], MOVE);
    line.animate([{ opacity: 1 }, { opacity: 0 }], MOVE);
    a.onfinish = a.oncancel = gone;
  }
}

// ---------- effects: what a file asks for, for as long as it asks ----------
function effectFrames(t, e, W_, H_) {
  const box = e._box || { x: 0, y: 0, w: 0, h: 0 },
    id = t.preset,
    sub = t.sub;
  if (id === 2 || id === 42 || id === 47 || id === 45) {
    const side = id === 2 ? sub : 4;
    const dx = side === 8 ? -(box.x + box.w) : side === 2 ? W_ - box.x : 0,
      dy = side === 1 ? -(box.y + box.h) : side === 4 ? (id === 2 ? H_ - box.y : 40) : 0;
    return [
      { translate: `${dx}px ${dy}px`, opacity: id === 2 ? 1 : 0 },
      { translate: '0px 0px', opacity: 1 },
    ];
  }
  if (id === 22 || /wipe/.test(t.filter || '')) {
    const dir = /\((\w+)\)/.exec(t.filter || '')?.[1] || 'left',
      from =
        dir === 'left'
          ? 'inset(0 100% 0 0)'
          : dir === 'right'
            ? 'inset(0 0 0 100%)'
            : dir === 'up'
              ? 'inset(0 0 100% 0)'
              : 'inset(100% 0 0 0)';
    return [{ clipPath: from }, { clipPath: 'inset(0 0 0 0)' }];
  }
  if (id === 23 || id === 53 || id === 21)
    return [
      { scale: 0, opacity: id === 53 ? 0 : 1 },
      { scale: 1, opacity: 1 },
    ];
  return [{ opacity: 0 }, { opacity: 1 }];
}
function playEffect(t, e) {
  if (reduce || t.preset === 1 || !e.animate) return null;
  const f = effectFrames(t, e.closest('[data-sp]') || e, deck.w, deck.h),
    dur = t.dur || 500;
  return e.animate(t.cls === 'exit' ? [...f].reverse() : f, {
    duration: dur,
    delay: t.wait || 0,
    easing: t.cls === 'exit' ? 'cubic-bezier(.4,0,.8,.3)' : EASE,
    fill: t.cls === 'exit' ? 'forwards' : 'backwards',
  });
}

// ---------- the panel for motion: built once, then only what changed moves ----------
const TRANS = [
    ['', '无效果'],
    ['fade', '淡入'],
    ['push', '推入'],
    ['cover', '覆盖'],
    ['wipe', '擦除'],
    ['zoom', '缩放'],
  ],
  DIRS = [
    ['l', '向左'],
    ['r', '向右'],
    ['u', '向上'],
    ['d', '向下'],
  ],
  SPEED = [
    [300, '快'],
    [500, '适中'],
    [1000, '慢'],
  ],
  WHEN = [
    ['click', '单击时'],
    ['with', '与上一动画同时'],
    ['after', '在上一动画之后'],
  ];
// a choice is a sliding capsule, in one row or two
function pickGroup(opts, idp, cols, pick) {
  const box = el('div', 'seg ax-pick' + (opts.length > cols ? ' rows' : '')),
    pl = el('i', 'pl');
  box.style.setProperty('--c', cols);
  box.appendChild(pl);
  const btns = opts.map(([k, tx]) => {
    const b = el('button', 'sg', T(tx));
    b.type = 'button';
    b.id = idp + '-' + (k === '' ? 'none' : k);
    b.dataset.k = k;
    b.disabled = locked;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      pick(k);
    });
    box.appendChild(b);
    return b;
  });
  let cur = null,
    seen = false;
  // when an option's words don't fit its column (Transición de diapositiva), fewer columns and the options in rows, as the
  // panel's longer groups already are (the layout adapts to the sentence)
  const textW = (b) => {
    const r = document.createRange();
    r.selectNodeContents(b);
    return r.getBoundingClientRect().width;
  };
  const fit = () => {
    if (!box.offsetParent) return;
    const room = box.clientWidth - 6;
    let c = cols;
    while (c > 1 && btns.some((b) => textW(b) + 6 > room / c)) c--;
    if (String(c) !== box.style.getPropertyValue('--c')) {
      box.style.setProperty('--c', c);
      box.classList.toggle('rows', opts.length > c);
    }
  };
  new ResizeObserver(() => {
    fit();
    place();
  }).observe(box);
  const place = () => {
    fit();
    const on = btns.find((b) => String(b.dataset.k) === String(cur));
    btns.forEach((b) => b.setAttribute('aria-pressed', String(b === on)));
    if (!on || !box.offsetParent) {
      pl.style.opacity = on ? '' : 0;
      return;
    }
    if (!seen) {
      pl.style.transition = 'none';
      seen = true;
    }
    pl.style.opacity = '';
    pl.style.transform = `translate(${on.offsetLeft}px, ${on.offsetTop}px)`;
    pl.style.width = on.offsetWidth + 'px';
    pl.style.height = on.offsetHeight + 'px';
    if (pl.style.transition === 'none') {
      void pl.offsetWidth;
      pl.style.transition = '';
    }
  };
  return {
    box,
    place,
    set(v) {
      cur = v;
      place();
    },
    reset() {
      seen = false;
    },
  };
}
const labelled = (label, g) => {
  const w = el('div');
  w.append(el('span', 'side-label', T(label)), g.box);
  return w;
};
// room opens and closes in --t-2, the new thing fading in
function folding(inner) {
  const f = el('div', 'ax-fold'),
    i = el('div', 'ax-in');
  i.appendChild(inner);
  f.appendChild(i);
  f.hidden = true;
  f._shown = false;
  return f;
}
function reveal(f, on) {
  if (f._shown === on) return;
  f._shown = on;
  f.getAnimations().forEach((a) => a.cancel());
  if (reduce || !animSide.classList.contains('on')) {
    f.hidden = !on;
    return;
  }
  if (on) {
    f.hidden = false;
    const h = f.offsetHeight;
    f.style.overflow = 'hidden';
    const a = f.animate(
      [
        { height: 0, opacity: 0 },
        { height: h + 'px', opacity: 1 },
      ],
      MOVE
    );
    a.onfinish = a.oncancel = () => {
      f.style.overflow = '';
    };
  } else {
    const h = f.offsetHeight;
    f.style.overflow = 'hidden';
    const a = f.animate(
      [
        { height: h + 'px', opacity: 1 },
        { height: 0, opacity: 0 },
      ],
      MOVE
    );
    a.onfinish = () => {
      f.hidden = true;
      f.style.overflow = '';
    };
    a.oncancel = () => {
      f.style.overflow = '';
    };
  }
}
let ax = null,
  axTab = 'page';
function buildAnim(body) {
  const tabs = pickGroup(
    [
      ['page', '页面切换'],
      ['elem', '对象动画'],
    ],
    'axTab',
    2,
    (k) => setAxTab(k)
  );
  tabs.box.querySelectorAll('.sg').forEach((b) => {
    b.disabled = false;
  });
  const change = (k, d, v) => {
    E_setTransition(curSlide(), k, d, v);
    deck.touched.add(curSlide().part);
    commit();
  };
  const nowTr = () => {
    const tr = curSlide().trans;
    return {
      kind: tr && tr.kind !== 'cut' ? tr.kind : '',
      dir: tr && /^[lrud]$/.test(tr.dir) ? tr.dir : 'l',
      spd: tr ? (tr.dur <= 400 ? 300 : tr.dur >= 900 ? 1000 : 500) : 500,
    };
  };
  const trans = pickGroup(TRANS, 'trans', 3, (k) => {
    const q = nowTr();
    change(k, q.dir, q.spd);
    if (k) previewTransition();
  });
  const dir = pickGroup(DIRS, 'dir', 4, (d) => {
    const q = nowTr();
    change(q.kind, d, q.spd);
    previewTransition();
  });
  const tspeed = pickGroup(SPEED, 'tspeed', 3, (v) => {
    const q = nowTr();
    change(q.kind, q.dir, v);
    previewTransition();
  });
  const all = el('button', 'ax-apply', T('应用至全部幻灯片'));
  all.type = 'button';
  all.id = 'transAll';
  all.disabled = locked;
  all.addEventListener('mousedown', (e) => e.preventDefault());
  all.addEventListener('click', () => {
    sound();
    const q = nowTr();
    deck.slides.forEach((x) => {
      E_setTransition(x, q.kind, q.dir, q.spd);
      deck.touched.add(x.part);
    });
    commit();
  });
  const nowFx = () => {
    const mine = selId ? curSlide().steps.find((t) => t.sp === selId && t.cls === 'entr') : null;
    return {
      key: mine ? E_effectKey(mine) : '',
      when: mine ? mine.when : 'click',
      spd: mine ? (mine.dur <= 300 ? 300 : mine.dur >= 900 ? 1000 : 500) : 500,
    };
  };
  const setFx = (k, w, d) => {
    const q = curSlide(),
      id = selId;
    if (k) E_addEffect(q, id, k, w, d);
    else E_clearEffects(q, id);
    deck.touched.add(q.part);
    commit();
  };
  const fx = pickGroup(
    [['', '无效果'], ...['appear', 'fade', 'wipe', 'rise', 'left', 'right', 'zoom'].map((k) => [k, EFFECTS[k].name])],
    'fx',
    3,
    (k) => {
      const q = nowFx();
      setFx(k, q.when, q.spd);
      if (k) previewOne();
    }
  );
  const when = pickGroup(WHEN, 'when', 1, (w) => {
    const q = nowFx();
    setFx(q.key, w, q.spd);
  });
  const fspeed = pickGroup(SPEED, 'fspeed', 3, (v) => {
    const q = nowFx();
    setFx(q.key, q.when, v);
    previewOne();
  });
  const pageBox_ = el('div', 'ax-box'),
    elemBox = el('div', 'ax-box'),
    holder = el('div', 'ax-holder');
  const fDir = folding(labelled('方向', dir)),
    fSpeed = folding(labelled('速度', tspeed)),
    fFx = folding(labelled('入场方式', fx)),
    fWhen = folding(labelled('触发时机', when)),
    fFsp = folding(labelled('速度', fspeed));
  const orderList = el('div', 'step-list'),
    orderWrap = el('div');
  orderWrap.append(el('span', 'side-label', T('播放顺序')), orderList);
  const fOrder = folding(orderWrap);
  const transBox = el('div'),
    transHead = el('div', 'ax-head');
  transHead.append(el('span', 'side-label', T('切换效果')), all);
  transBox.append(transHead, trans.box);
  pageBox_.append(transBox, fDir, fSpeed);
  elemBox.append(fFx, fWhen, fFsp, fOrder);
  holder.append(pageBox_, elemBox);
  const preview = el('button', 'btn primary side-wide', T('预览当前页动画'));
  preview.type = 'button';
  preview.id = 'animPreview';
  preview.addEventListener('mousedown', (e) => e.preventDefault());
  preview.addEventListener('click', () => {
    sound();
    previewSlide();
  });
  body.append(tabs.box, holder, preview);
  ax = {
    tabs,
    trans,
    dir,
    tspeed,
    fx,
    when,
    fspeed,
    pageBox: pageBox_,
    elemBox,
    holder,
    fDir,
    fSpeed,
    all,
    fFx,
    fWhen,
    fFsp,
    fOrder,
    orderList,
    rows: new Map(),
    nowTr,
    nowFx,
  };
  elemBox.hidden = axTab !== 'elem';
  pageBox_.hidden = axTab !== 'page';
}
function setAxTab(k) {
  axTab = k;
  if (!ax) return;
  ax.tabs.set(k);
  const on = k === 'page' ? ax.pageBox : ax.elemBox,
    off = k === 'page' ? ax.elemBox : ax.pageBox;
  if (!on.hidden) {
    syncAnim();
    return;
  }
  shell.morph(
    ax.holder,
    () => {
      off.hidden = true;
      on.hidden = false;
      syncAnim();
    },
    () => [on]
  );
}
const placeListPill = (pl, row) => {
  if (!row) {
    pl.style.opacity = 0;
    return;
  }
  if (!row.offsetParent) return;
  const fresh = pl.style.opacity !== '1';
  if (fresh) pl.style.transition = 'opacity var(--t-1) var(--e-out)';
  pl.style.opacity = '1';
  pl.style.transform = `translateY(${row.offsetTop}px)`;
  pl.style.height = row.offsetHeight + 'px';
  if (fresh) {
    void pl.offsetWidth;
    pl.style.transition = '';
  }
};
// a step's name in the order list: its own paragraphs' words when it plays one paragraph at a time, never the bullets
const stepText = (host, para) => {
  const ps = [...host.querySelectorAll('p')].filter(
    (p) => !para || (+p.dataset.p >= para[0] && +p.dataset.p <= para[1])
  );
  const words = (p) => {
    const c = p.cloneNode(true);
    c.querySelectorAll('.x-bu').forEach((b) => b.remove());
    return c.textContent;
  };
  return (ps.length ? ps.map(words).join(' ') : host.textContent || '').replace(/[\s\u200b]+/g, ' ').trim();
};
function syncAnim() {
  if (!ax) return;
  ax.tabs.set(axTab);
  const q = ax.nowTr(),
    f = ax.nowFx(),
    directional = q.kind && q.kind !== 'fade' && q.kind !== 'zoom',
    s = curSlide();
  ax.trans.set(q.kind);
  reveal(ax.fDir, !!directional);
  reveal(ax.fSpeed, !!q.kind);
  ax.all.classList.toggle('on', !!q.kind && !locked);
  ax.dir.set(q.dir);
  ax.tspeed.set(q.spd);
  reveal(ax.fFx, !!selEl);
  ax.fx.set(f.key);
  reveal(ax.fWhen, !!selEl && !!f.key);
  reveal(ax.fFsp, !!selEl && !!f.key);
  ax.when.set(f.when);
  ax.fspeed.set(f.spd);
  {
    const others = s.steps.some((t) => t.sp !== selId && (t.cls === 'entr' || t.cls === 'exit'));
    ['with', 'after'].forEach((k) => {
      const b = ax.when.box.querySelector('#when-' + k);
      if (b) b.disabled = locked || !others;
    });
  }
  // the order: rows keep their place on screen and slide to the new one
  const items = s.steps.filter((t) => t.cls === 'entr' || t.cls === 'exit'),
    first = new Map([...ax.rows].map(([k, r]) => [k, r.getBoundingClientRect()])),
    keep = new Set();
  reveal(ax.fOrder, items.length > 0);
  // a shape whose points play one by one has several rows: the pill lights only its first, so the others keep their ink
  let lit = false;
  items.forEach((t, i) => {
    const key = t.sp + '/' + (t.para ? t.para[0] : ''),
      host =
        pages[nowAt].inner.lastElementChild && pages[nowAt].inner.lastElementChild.querySelector(`[data-sp="${t.sp}"]`),
      name = host && host._node ? stepText(host, t.para).slice(0, 14) || nameOf_(host._node) : t.sp;
    let r = ax.rows.get(key);
    if (!r) {
      r = el('button', 'step-row');
      r.type = 'button';
      const sn = el('span', 'step-n');
      sn.dataset.file = '';
      r.append(el('span', 'step-no'), sn);
      r.addEventListener('mousedown', (e) => e.preventDefault());
      r.addEventListener('click', () => {
        sound();
        const top = topAt(t.sp);
        if (top && !locked) selectEl(top);
      });
      const mv = (ic, d) => {
        const b = el('i', 'step-mv');
        b.innerHTML = icon(ic);
        b.addEventListener('click', (e) => {
          e.stopPropagation();
          sound();
          if (!locked && E_moveClick(curSlide(), t.sp, d)) {
            deck.touched.add(curSlide().part);
            commit();
          }
        });
        return b;
      };
      r.append(mv('up', -1), mv('down', 1));
      ax.rows.set(key, r);
      if (!reduce) r.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
    }
    r.id = 'step-' + i;
    r.children[0].textContent = t.when === 'click' ? String(t.step) : t.when === 'with' ? '+' : '→';
    r.children[1].textContent = name;
    r.setAttribute('aria-pressed', String(t.sp === selId && !lit));
    if (t.sp === selId) lit = true;
    r.classList.toggle('no-mv', t.when !== 'click' || locked);
    ax.orderList.appendChild(r);
    keep.add(key);
  });
  for (const [k, r] of ax.rows)
    if (!keep.has(k)) {
      ax.rows.delete(k);
      r.remove();
    }
  if (!reduce)
    for (const [k, r] of ax.rows) {
      const a = first.get(k);
      if (!a) continue;
      const b = r.getBoundingClientRect();
      if (Math.abs(a.top - b.top) > 0.5 && a.height)
        r.animate([{ transform: `translateY(${a.top - b.top}px)` }, { transform: 'none' }], MOVE);
    }
  if (!ax.orderPill || !ax.orderPill.isConnected) {
    ax.orderPill = el('i', 'list-pl');
    ax.orderList.prepend(ax.orderPill);
  }
  placeListPill(
    ax.orderPill,
    [...ax.rows.values()].find((r) => r.getAttribute('aria-pressed') === 'true')
  );
  [ax.tabs, ax.trans, ax.dir, ax.tspeed, ax.fx, ax.when, ax.fspeed].forEach((g) => g.place());
}
const nameOf_ = (n) =>
  str(
    kid(
      kids(n).find((x) => /^nv/.test(x.localName)),
      'cNvPr'
    ),
    'name'
  ) || '';
let previewToken = 0;
function stopPreview() {
  previewToken++;
  const p = pages[nowAt];
  if (p) {
    p.inner.getAnimations({ subtree: true }).forEach((a) => a.cancel());
    p.inner.querySelectorAll('[data-sp], p').forEach((e) => {
      e.style.visibility = '';
    });
  }
}
function previewTransition() {
  const p = pages[nowAt],
    tr = curSlide().trans;
  if (!tr || reduce) return;
  const k = p.el.offsetWidth;
  const f = transFrames(tr, k, p.el.offsetHeight);
  p.inner.animate(
    f.inFrames.map((x) => ({ ...x, transform: `${x.transform || ''} scale(${curK()})` })),
    { duration: tr.dur, easing: EASE }
  );
}
function previewOne() {
  const s = curSlide(),
    t = s.steps.find((x) => x.sp === selId && x.cls === 'entr'),
    e = selEl;
  if (t && e) {
    frame.style.visibility = 'hidden';
    const a = playEffect({ ...t, wait: 0 }, e);
    const end = () => {
      frame.style.visibility = '';
    };
    if (a) a.onfinish = a.oncancel = end;
    else end();
  }
}
async function previewSlide() {
  const s = curSlide(),
    token = ++previewToken,
    p = pages[nowAt],
    slide = p.inner.firstElementChild;
  if (!slide) return;
  clearSelection();
  applySteps(nowAt, slide, 0, false);
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  if (s.trans) {
    previewTransition();
    await wait(s.trans.dur);
  }
  for (let i = 1; i <= s.stepCount; i++) {
    await wait(500);
    if (token !== previewToken) return;
    applySteps(nowAt, slide, i, true);
    const longest = s.steps
      .filter((t) => t.step === i)
      .reduce((a, t) => Math.max(a, (t.dur || 500) + (t.wait || 0)), 0);
    await wait(longest);
    if (token !== previewToken) return;
  }
  await wait(400);
  if (token === previewToken)
    slide.querySelectorAll('[data-sp], p').forEach((e) => {
      e.style.visibility = '';
    });
}
function transFrames(tr, W_, H_) {
  const d = tr.dir,
    dx = d === 'r' ? W_ : d === 'l' || d === 'lu' || d === 'ld' ? -W_ : 0,
    dy = d === 'd' ? H_ : d === 'u' ? -H_ : 0;
  if (tr.kind === 'push')
    return {
      inFrames: [{ transform: `translate(${-dx}px, ${-dy}px)` }, { transform: 'translate(0, 0)' }],
      outFrames: [{ transform: 'translate(0, 0)' }, { transform: `translate(${dx}px, ${dy}px)` }],
    };
  if (tr.kind === 'cover')
    return { inFrames: [{ transform: `translate(${-dx}px, ${-dy}px)` }, { transform: 'translate(0, 0)' }] };
  if (tr.kind === 'wipe') {
    const from =
      d === 'l'
        ? 'inset(0 0 0 100%)'
        : d === 'r'
          ? 'inset(0 100% 0 0)'
          : d === 'u'
            ? 'inset(100% 0 0 0)'
            : 'inset(0 0 100% 0)';
    return { inFrames: [{ clipPath: from }, { clipPath: 'inset(0 0 0 0)' }] };
  }
  if (tr.kind === 'zoom')
    return {
      inFrames: [
        { transform: 'scale(.86)', opacity: 0 },
        { transform: 'scale(1)', opacity: 1 },
      ],
    };
  return { inFrames: [{ opacity: 0 }, { opacity: 1 }] };
}

// ---------- the pages view's own work: picking pages ----------
let picked = new Set();
function syncPicked() {
  const n = picked.size,
    info = n ? T('已选 {n} 页', { n }) : T('选择幻灯片');
  pages.forEach((p) => p.el.classList.toggle('on', picked.has(p)));
  if (pickInfo.textContent !== info) {
    pickInfo.textContent = info;
    if (!reduce) pickInfo.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  }
  [dupPages, hidePages, delPages].forEach((b) => {
    b.disabled = locked || !n;
  });
  const allHid = n && [...picked].every((p) => deck.slides[p.i].hidden);
  hideLab.textContent = T(allHid ? '取消隐藏' : '隐藏此页');
}
function flipPages(change, skip) {
  const first = new Map(pages.map((p) => [p, p.el.getBoundingClientRect()])),
    tile0 = addTile.getBoundingClientRect();
  change();
  if (reduce) return;
  const tile1 = addTile.getBoundingClientRect();
  if (tile0.width && tile1.width && (Math.abs(tile0.left - tile1.left) > 0.5 || Math.abs(tile0.top - tile1.top) > 0.5))
    addTile.animate(
      [{ transform: `translate(${tile0.left - tile1.left}px, ${tile0.top - tile1.top}px)` }, { transform: 'none' }],
      MOVE
    );
  for (const p of pages) {
    if (p === skip) continue;
    const a = first.get(p),
      b = p.el.getBoundingClientRect();
    if (!a) {
      p.el.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
      continue;
    }
    if (Math.abs(a.left - b.left) < 0.5 && Math.abs(a.top - b.top) < 0.5) continue;
    p.el.animate([{ transform: `translate(${a.left - b.left}px, ${a.top - b.top}px)` }, { transform: 'none' }], MOVE);
  }
}
function deletePicked() {
  if (locked || !picked.size) return;
  if (picked.size >= pages.length) {
    say('演示文稿至少需保留一页幻灯片。');
    return;
  }
  const gone = pages.filter((p) => picked.has(p));
  if (!reduce)
    gone.forEach((p) => {
      const r = p.el.getBoundingClientRect(),
        g = p.el.cloneNode(true);
      g.classList.add('pg-ghost');
      g.classList.remove('on', 'cur');
      g.style.cssText += `;left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px;margin:0`;
      shell.app.insertBefore(g, stage.nextSibling);
      const a = g.animate(
        [
          { opacity: 1, transform: 'none' },
          { opacity: 0, transform: 'scale(.92)' },
        ],
        MOVE
      );
      a.onfinish = () => g.remove();
    });
  gone.forEach((p) => E_removeSlide(deck, deck.slides[p.i]));
  noteGone(gone);
  picked = new Set();
  flipPages(() => commit());
}
function noteGone(list_) {
  for (const p of list_) {
    const s = deck.slides[p.i];
    deck.gone.add(s.part);
    deck.gone.add(deck.relsPath(s.part));
    const n = deck.relOfType(s.rels, 'notesSlide');
    if (n) {
      deck.gone.add(n.target);
      deck.gone.add(deck.relsPath(n.target));
    }
  }
}
function duplicatePicked() {
  if (locked || !picked.size) return;
  const list_ = pages.filter((p) => picked.has(p));
  const after = Math.max(...list_.map((p) => p.i)) + 1;
  list_.reverse().forEach((p) => E_duplicateSlide(deck, deck.slides[p.i], after));
  picked = new Set();
  flipPages(() => commit());
}
function hidePicked() {
  if (locked || !picked.size) return;
  const list_ = [...picked],
    allHid = list_.every((p) => deck.slides[p.i].hidden);
  list_.forEach((p) => E_setHidden(deck, deck.slides[p.i], !allHid));
  commit();
}
// a layout's own name is the file's; one with none is called Layout in the window's language
function addPage(at) {
  if (locked) return;
  const m = curSlide().master,
    layouts = [...m.rels.values()]
      .filter((r) => r.type === 'slideLayout')
      .map((r) => {
        const n = str(kid(deck.xml(r.target), 'cSld'), 'name');
        return { value: r.target, label: n || T('页面版式'), raw: !!n };
      });
  shell.openPop(
    addBlank,
    layouts,
    (path) => {
      flipPages(() => {
        const part = E_addSlide(deck, path, at);
        commit();
        if (!pagesMode) {
          const i = pages.findIndex((p) => p.part === part);
          if (i >= 0) goPage(i);
        }
      });
    },
    { up: true }
  );
}
let pageDrag = null;
list.addEventListener(
  'pointerdown',
  (e) => {
    if (!pagesMode || e.button || e.target.closest('.add-tile')) return;
    const g = e.target.closest('.pg');
    if (!g) {
      picked = new Set();
      syncPicked();
      return;
    }
    e.preventDefault();
    pageDrag = { pg: pages[+g.dataset.i], sx: e.clientX, sy: e.clientY, moved: false };
  },
  true
);
addEventListener('pointermove', (e) => {
  const d = pageDrag;
  if (!d) return;
  const dx = e.clientX - d.sx,
    dy = e.clientY - d.sy;
  if (locked) return;
  if (!d.moved) {
    if (Math.hypot(dx, dy) < 5) return;
    d.moved = true;
    d.pg.el.classList.add('lifted');
    d.group = picked.has(d.pg) && picked.size > 1 ? pages.filter((p) => picked.has(p)) : [d.pg];
  }
  d.pg.el.style.transform = `translate(${dx}px, ${dy}px)`;
  const rest = pages.filter((p) => !d.group.includes(p));
  let at = rest.length;
  for (let i = 0; i < rest.length; i++) {
    const r = rest[i].el.getBoundingClientRect();
    if ((e.clientY < r.bottom + 20 && e.clientX < r.left + r.width / 2) || e.clientY < r.top) {
      at = i;
      break;
    }
  }
  const next = [...rest.slice(0, at), ...d.group, ...rest.slice(at)];
  if (next.some((p, i) => p !== pages[i])) {
    const before = d.pg.el.getBoundingClientRect();
    flipPages(() => {
      pages = next;
      pages.forEach((p) => list.insertBefore(p.el, addTile));
    }, d.pg);
    const after = d.pg.el.getBoundingClientRect();
    d.sx += after.left - before.left;
    d.sy += after.top - before.top;
    d.pg.el.style.transform = `translate(${e.clientX - d.sx}px, ${e.clientY - d.sy}px)`;
    d.changed = true;
  }
});
addEventListener('pointerup', () => {
  const d = pageDrag;
  if (!d) return;
  pageDrag = null;
  if (!d.moved) {
    if (picked.has(d.pg)) picked.delete(d.pg);
    else picked.add(d.pg);
    syncPicked();
    return;
  }
  d.pg.el.classList.remove('lifted');
  const t = d.pg.el.style.transform;
  d.pg.el.style.transform = '';
  if (!reduce && t) d.pg.el.animate([{ transform: t }, { transform: 'none' }], MOVE);
  if (d.changed) {
    E_moveSlides(
      deck,
      pages.map((p) => p.i)
    );
    commit();
  }
});

// ---------- save as: the family's card with this tool's rows ----------
let sizeKey = 'same',
  rangeKey = 'all',
  sharpKey = '1920',
  withHidden = false;
const SIZES = { same: ['原始品质', 1], small: ['均衡压缩', 0.58], least: ['极致压缩', 0.34] },
  SHARP = { 1280: '1280', 1920: '1920', 3840: '3840' };
const shownList = () =>
  rangeKey === 'one' ? [nowAt] : pages.map((_, i) => i).filter((i) => withHidden || !deck.slides[i].hidden);
function estimate(fmt) {
  if (!deck) return 0;
  const rest = Math.max(0, fileSize - deck.mediaBytes),
    f = SIZES[sizeKey][1];
  if (fmt === 'pptx') return rest + deck.mediaBytes;
  // more than one picture says how many
  if (fmt === 'png') {
    const n = shownList().length;
    return { size: { 1280: 180, 1920: 340, 3840: 1000 }[sharpKey] * 1024 * n, note: n > 1 && T('，{n} 张', { n }) };
  }
  return (rest * 0.4 + deck.mediaBytes * f) * (shownList().length / pages.length);
}
function chipRow(label, map, key, set, again) {
  const box = el('div'),
    chips = el('div', 'chips three');
  Object.entries(map).forEach(([k, v]) => {
    const t = Array.isArray(v) ? v[0] : v,
      b = el('button', 'chip dyn', T(t));
    b.type = 'button';
    b.id = 'chip-' + k;
    b.setAttribute('aria-pressed', String(k === key));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      set(k);
      [...chips.children].forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      again();
    });
    chips.appendChild(b);
  });
  box.append(el('span', 'side-label dyn', T(label)), chips);
  return box;
}
function rangeRow(rows, again) {
  const box = el('div'),
    seg = el('div', 'seg'),
    pl = el('i', 'pl');
  seg.appendChild(pl);
  const btns = [
    ['all', '全部幻灯片'],
    ['one', '页面切换'],
  ].map(([k, t]) => {
    const b = el('button', 'sg dyn', T(t));
    b.type = 'button';
    b.id = 'range-' + k;
    b.setAttribute('aria-pressed', String(k === rangeKey));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      rangeKey = k;
      btns.forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      place();
      again();
    });
    seg.appendChild(b);
    return b;
  });
  const place = () => {
    const on = btns.find((b) => b.getAttribute('aria-pressed') === 'true');
    pl.style.transform = `translateX(${on.offsetLeft}px)`;
    pl.style.width = on.offsetWidth + 'px';
  };
  box.append(el('span', 'side-label dyn', T('导出范围')), seg);
  rows.appendChild(box);
  pl.style.transition = 'none';
  place();
  void pl.offsetWidth;
  pl.style.transition = '';
  requestAnimationFrame(place);
}
function drawCard(rows, fmt, again) {
  flushNotes();
  if (editingText) finishText();
  if (fmt !== 'pptx') rangeRow(rows, again);
  if (fmt === 'png')
    rows.appendChild(
      chipRow(
        '画面清晰度',
        SHARP,
        sharpKey,
        (k) => {
          sharpKey = k;
        },
        again
      )
    );
  else if (fmt === 'pdf')
    rows.appendChild(
      chipRow(
        '文件大小',
        SIZES,
        sizeKey,
        (k) => {
          sizeKey = k;
        },
        again
      )
    );
  if (fmt !== 'pptx' && deck.slides.some((s) => s.hidden)) {
    const t = el('button', 'row compact');
    t.type = 'button';
    t.id = 'withHidden';
    t.setAttribute('role', 'switch');
    t.setAttribute('aria-checked', String(withHidden));
    t.innerHTML = `<span class="row-t"><b>${T('包含已隐藏的幻灯片')}</b></span><i class="tog"></i>`;
    t.addEventListener('mousedown', (e) => e.preventDefault());
    t.addEventListener('click', () => {
      sound();
      withHidden = !withHidden;
      t.setAttribute('aria-checked', String(withHidden));
      again();
    });
    rows.appendChild(t);
  }
}
async function saveTo(path, fmt) {
  flushNotes();
  if (editingText) finishText();
  stopMedia();
  if (fmt === 'pptx') {
    // a pptm, ppsm or potm is only read: what's saved is a pptx, without its macros
    if (READ_ONLY.includes(kindOf)) {
      dropMacros(deck);
      setMainType(deck, 'pptx');
    }
    let bytes;
    try {
      bytes = saveDeck(deck);
    } catch (e) {
      return { status: 'failed', kind: 'other', detail: String(e) };
    }
    const r = await own.saveBytes(path, bytes, false);
    if (r.status === 'saved') r.size = bytes.length;
    return r;
  }
  const list_ = shownList();
  let r;
  if (fmt === 'pdf') r = await printPdf(path, list_);
  // the Mac: the same print layout, each sheet drawn as a picture by the system (src-tauri/src/mac.rs, pdf_pictures)
  else if (platform === 'mac') r = await printPdf(path, list_, +sharpKey);
  else r = await exportPng(deck, list_, +sharpKey, path, own.saveBytes);
  return r;
}
function afterSave(r, fmt) {
  if (r.exported) return;
  savedAt = hAt;
  fileSize = r.size || fileSize;
  if (locked) {
    locked = false;
    macros = false;
    kindOf = 'pptx';
    hideLock();
    syncLocked();
  }
  kindOf = (r.name.split('.').pop() || 'pptx').toLowerCase();
}

// ---------- a PDF: the slides laid out one to a sheet the slide's own size, printed by WebView2 itself ----------
const printBox = el('div', 'print-deck');
document.body.appendChild(printBox);
// The Mac's PNG (`pixels` wide) is printed the same way, with the pictures as they are, each slide on a sheet a pixel
// taller than it (the sheet keeps a pixel back so that nothing spills onto another sheet): the whole slide is drawn.
async function printPdf(path, list_, pixels) {
  const k = 96 / 72,
    f = pixels ? 1 : SIZES[sizeKey][1];
  printBox.textContent = '';
  printBox.style.setProperty('--pw', deck.w * k + 'px');
  printBox.style.setProperty('--ph', deck.h * k + (pixels ? 1 : 0) + 'px');
  const shrunk = [];
  for (const i of list_) {
    const sheet = el('div', 'print-sheet'),
      s = paintSlide(deck, i);
    s.style.transformOrigin = '0 0';
    s.style.transform = `scale(${k})`;
    sheet.appendChild(s);
    printBox.appendChild(sheet);
    if (f < 1) shrunk.push(...(await shrinkPictures(s, f)));
  }
  await Promise.all(
    [...printBox.querySelectorAll('img')].map((im) =>
      im.complete
        ? null
        : new Promise((ok) => {
            im.onload = im.onerror = ok;
          })
    )
  );
  if (document.fonts && document.fonts.ready) await document.fonts.ready;
  if (pixels)
    try {
      return await own.printPng(pngPaths(path, list_.length), deck.w / 72, deck.h / 72, pixels);
    } catch (e) {
      return { status: 'failed', kind: 'picture', detail: String(e) };
    } finally {
      printBox.textContent = '';
    }
  // the PDF's own title is the deck's name
  const title = document.title;
  document.title = (path.split(/[\\/]/).pop() || '').replace(/\.pdf$/i, '');
  try {
    return await own.printPdf(path, deck.w / 72, deck.h / 72);
  } finally {
    document.title = title;
    printBox.textContent = '';
    shrunk.forEach((u) => URL.revokeObjectURL(u));
  }
}
// the Balanced and Compact sizes: pictures redrawn smaller (but never smaller than they're shown) and stored as JPEG,
// unless they're see-through
async function shrinkPictures(slide, f) {
  const made = [];
  for (const im of slide.querySelectorAll('img')) {
    if (!im.src.startsWith('blob:')) continue;
    try {
      const bmp = await createImageBitmap(await (await fetch(im.src)).blob()),
        w0 = bmp.width,
        h0 = bmp.height,
        shown = Math.max(parseFloat(im.style.width) || 0, 1) * 2;
      const k = Math.min(1, Math.max(f, shown / w0));
      if (k >= 1 && f === 1) continue;
      const c = document.createElement('canvas');
      c.width = Math.max(1, Math.round(w0 * k));
      c.height = Math.max(1, Math.round(h0 * k));
      const g = c.getContext('2d');
      g.drawImage(bmp, 0, 0, c.width, c.height);
      const px = g.getImageData(0, 0, c.width, c.height).data;
      let clear = false;
      for (let j = 3; j < px.length; j += 4 * 7)
        if (px[j] < 250) {
          clear = true;
          break;
        }
      const blob = await new Promise((ok) => c.toBlob(ok, clear ? 'image/png' : 'image/jpeg', f > 0.5 ? 0.82 : 0.7));
      const u = URL.createObjectURL(blob);
      made.push(u);
      im.src = u;
    } catch {
      /* this one stays as it is */
    }
  }
  return made;
}

// ---------- opening: the bytes from Rust, read by deck.js, then adopted ----------
async function openDeck(path) {
  let bytes;
  try {
    bytes = new Uint8Array(await own.readDeck(path));
  } catch (e) {
    return { status: String(e) === 'missing' || String(e) === 'refused' ? String(e) : 'damaged' };
  }
  const ext = (path.split('.').pop() || '').toLowerCase();
  let d;
  try {
    d = readDeck(bytes);
    if (!d.slides.length) throw new Error('no slides');
  } catch (e) {
    return { status: e instanceof Encrypted ? 'encrypted' : 'damaged' };
  }
  const r = await own.adoptDeck(path);
  pending = { deck: d, r: { ...r, ext } };
  return r;
}
function holdDeck(d, r) {
  shell.closeAll();
  if (showing) endShow();
  stopMedia();
  if (deck) for (const u of deck.urls.values()) if (u) URL.revokeObjectURL(u);
  deck = d;
  fileName = r.name;
  fileSize = r.size;
  opened = true;
  nowAt = 0;
  pagesMode = false;
  root.classList.remove('pages-mode');
  kindOf = r.ext in KIND ? r.ext : 'pptx';
  // pptm, ppsm, potm: their macros are never run (nothing in a file is ever run) and they're only read; pptx, ppsx
  // and potx are edited
  macros = READ_ONLY.includes(kindOf) || !!Object.keys(d.files).find((n) => /vbaProject\.bin$/i.test(n));
  locked = READ_ONLY.includes(kindOf);
  syncKind();
  syncLocked();
  root.style.setProperty('--slide-ratio', d.w / d.h);
  near.disconnect();
  list.textContent = '';
  pages = d.slides.map((_, i) => makePage(i));
  pages.forEach((p) => list.appendChild(p.el));
  list.appendChild(addTile);
  selEl = null;
  selId = null;
  editingText = null;
  frame.remove();
  hideBar();
  startHistory();
  panelShown = '';
  layout();
  syncPageNo();
  pagesBtn.setAttribute('aria-pressed', 'false');
  if (panelOn()) renderPanel();
  stage.scrollTop = 0;
  stage.focus({ preventScroll: true });
  if (!reduce) pages[0].el.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
  if (locked) showLock();
  else hideLock();
}
// the save card's file kind follows the file: a ppsx saves as a ppsx, a pptm, ppsm or potm as a pptx
function syncKind() {
  const [ext, label] = KIND[kindOf] || KIND.pptx;
  FORMATS.pptx.ext = ext;
  FORMATS.pptx.label = label;
  const b = document.getElementById('fmt-pptx');
  if (b) b.textContent = label;
}
function syncLocked() {
  root.classList.toggle('locked', locked);
  [addTextBtn, addPicBtn, addShapeBtn, addMediaBtn, addBlank].forEach((b) => {
    b.disabled = locked;
  });
  syncUndo();
  syncPicked();
  if (notesBox) {
    notesBox.readOnly = locked;
  }
}
function goEmpty() {
  shell.closePop();
  if (showing) endShow();
  stopMedia();
  if (deck) for (const u of deck.urls.values()) if (u) URL.revokeObjectURL(u);
  opened = false;
  deck = null;
  pages = [];
  list.textContent = '';
  fileName = '';
  pagesMode = false;
  locked = false;
  macros = false;
  kindOf = 'pptx';
  root.classList.remove('pages-mode', 'locked');
  hideLock();
  panelShown = '';
  Object.values(SIDES).forEach((s) => {
    s.textContent = '';
  });
  hAt = savedAt = 0;
  history = [];
}

// ---------- a format that's only read: the family's notice row, saying so ----------
const lock = shell.notice(
  () =>
    macros
      ? '当前格式仅支持阅览与放映，如需修改请另存为 PPTX。注意：原文件中的宏脚本不会运行，亦不会写入新文档。'
      : '当前格式仅支持阅览与放映，如需修改请另存为 PPTX。',
  [
    ['另存为…', () => shell.openSaveAs(), 'lockSaveAs'],
    ['知道了', null, 'lockOk'],
  ],
  { id: 'lockNotice' }
);
const showLock = () => lock.show(),
  hideLock = () => lock.hide();

// ---------- keys ----------
let nudgeT = 0;
// before the family's: typing in a slide and the show own every key while they last
addEventListener(
  'keydown',
  (e) => {
    const mod = modKey(e),
      k = e.key;
    if (editingText) {
      const stop = () => {
        e.preventDefault();
        e.stopImmediatePropagation();
      };
      if (k === 'Escape') {
        stop();
        finishText();
        return;
      }
      if (mod && k.toLowerCase() === 'b') {
        stop();
        bar.querySelector('#edBold').click();
        return;
      }
      if (mod && k.toLowerCase() === 'i') {
        stop();
        bar.querySelector('#edItalic').click();
        return;
      }
      if (mod && k.toLowerCase() === 'u') {
        stop();
        bar.querySelector('#edUnder').click();
        return;
      }
      if (mod && k.toLowerCase() === 's') {
        stop();
        finishText();
        shell.openSaveAs();
        return;
      }
      if (k === 'Tab') e.preventDefault();
      e.stopImmediatePropagation();
      return;
    }
    if (showing) {
      // the Mac's ⌘ keys are the menu bar's even in a show (⌘W closes, ⌘H hides, ⌘, opens Settings), never W's white
      // screen
      if (platform === 'mac' && e.metaKey) return;
      e.stopImmediatePropagation();
      if (showKey(e)) {
        e.preventDefault();
        wakeHud();
      }
      return;
    }
  },
  true
);
function keyDown(e) {
  const mod = modKey(e),
    k = e.key,
    typing = /INPUT|TEXTAREA/.test(document.activeElement.tagName);
  if (!opened) return;
  if (k === 'F5') {
    e.preventDefault();
    startShow(e.shiftKey ? nowAt : visibleFrom(0, 1));
    return;
  }
  // the Mac's presentation keys: ⌘↩ from the start, ⇧⌘↩ from this slide (keys.js shows {Shift+F5} as ⇧⌘↩ there)
  if (platform === 'mac' && mod && k === 'Enter' && !typing && !e.altKey) {
    e.preventDefault();
    startShow(e.shiftKey ? nowAt : visibleFrom(0, 1));
    return;
  }
  if (k === 'Escape' && !typing) {
    if (selEl) {
      clearSelection();
      return;
    }
    if (pagesMode) leavePages();
    return;
  }
  if (typing || document.querySelector('.ask-wrap.on, .dlg.on, .set.on')) return;
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
  if (mod && k.toLowerCase() === 'd' && selEl) {
    e.preventDefault();
    duplicateSel();
    return;
  }
  if (mod && k.toLowerCase() === 'c' && selEl) {
    copySel(false);
    return;
  }
  if (mod && k.toLowerCase() === 'x' && selEl) {
    copySel(true);
    return;
  }
  if (mod) return;
  if (selEl) {
    if (k === 'Delete' || k === 'Backspace') {
      e.preventDefault();
      deleteSel();
      return;
    }
    if (k === 'F2' || k === 'Enter') {
      const q = selEl.querySelector('.x-t');
      if (q) {
        e.preventDefault();
        startText(selEl, q, { x: -1, y: -1 });
      }
      return;
    }
    const dx = k === 'ArrowRight' ? 1 : k === 'ArrowLeft' ? -1 : 0,
      dy = k === 'ArrowDown' ? 1 : k === 'ArrowUp' ? -1 : 0;
    if (dx || dy) {
      e.preventDefault();
      const st = e.shiftKey ? 10 : 1,
        b2 = { ...selEl._box, x: selEl._box.x + dx * st, y: selEl._box.y + dy * st };
      showBox(b2);
      E_setBox(selEl._node, b2);
      deck.touched.add(curSlide().part);
      clearTimeout(nudgeT);
      nudgeT = setTimeout(commit, 400);
      return;
    }
  }
  if ((k === 'Delete' || k === 'Backspace') && pagesMode) {
    e.preventDefault();
    deletePicked();
    return;
  }
  if (k === 'ArrowRight' || k === 'ArrowDown' || k === 'PageDown' || k === ' ') {
    e.preventDefault();
    goPage(nowAt + 1);
    return;
  }
  if (k === 'ArrowLeft' || k === 'ArrowUp' || k === 'PageUp') {
    e.preventDefault();
    goPage(nowAt - 1);
    return;
  }
  if (k === 'Home') {
    e.preventDefault();
    goPage(0);
    return;
  }
  if (k === 'End') {
    e.preventDefault();
    goPage(pages.length - 1);
    return;
  }
  if (k === 'Enter' && pagesMode) {
    e.preventDefault();
    leavePages();
  }
}

// ---------- language ----------
// names this program gives what it adds to a file, in the window's language (PowerPoint does the same)
const syncNames = () =>
  Object.assign(NAMES, {
    text: T('文本框::新对象的名称'),
    shape: T('形状::新对象的名称'),
    line: T('直线::新对象的名称'),
    picture: T('图像::新对象的名称'),
    video: T('视频::新对象的名称'),
    audio: T('音频::新对象的名称'),
    placeholder: T('占位符::新对象的名称'),
    notes: T('备注占位符::新对象的名称'),
    notesWord: T('演讲者备注::新对象的名称'),
    lang: langInfo().locale,
  });
shell.translates(pill, hud, pvSide, endCard);
shell.onLang(() => {
  syncNames();
  syncPicked();
  addBlank.querySelector('span').textContent = T('新建幻灯片');
  if (opened) {
    panelShown = '';
    renderPanel();
    drawBar();
    if (selEl) placeBar();
  }
});
syncNames();

// ---------- begin ----------
stage.tabIndex = -1;
show.tabIndex = -1;
// with DOTANY_TEST set, shell.test exposes the deck and its actions, so that an automated test can read and drive it
// (defineProperties, not assign: the getters stay live)
Object.defineProperties(
  shell.test,
  Object.getOwnPropertyDescriptors({
    get deck() {
      return deck;
    },
    get nowAt() {
      return nowAt;
    },
    get pages() {
      return pages;
    },
    get showAt() {
      return showAt;
    },
    get showStep() {
      return showStep;
    },
    get showing() {
      return showing;
    },
    get ended() {
      return ended;
    },
    get presenter() {
      return presenter;
    },
    get locked() {
      return locked;
    },
    get kindOf() {
      return kindOf;
    },
    get selEl() {
      return selEl;
    },
    get hAt() {
      return hAt;
    },
    get picked() {
      return picked;
    },
    get playing() {
      return playing;
    },
    get panelShown() {
      return panelShown;
    },
    flushNotes,
    undo,
    redo,
    commit,
    toggleMedia,
    insertBytes,
    goPage,
    enterPages,
    leavePages,
    startShow,
    advance,
    back,
    endShow,
    setPresenter,
    ensure,
    applyToText,
    finishText,
    syncPicked,
    selectId: (id) => {
      const e = topAt(id);
      if (e) selectEl(e);
      return !!e;
    },
    startTextOn: () => {
      const q = selEl.querySelector('.x-t');
      startText(selEl, q, { x: -1, y: -1 });
      return q;
    },
    pick: (is) => {
      picked = new Set(is.map((i) => pages[i]));
      syncPicked();
    },
    saveNow: () => saveDeck(deck),
    // a real change, for an automated test to make: the first page hidden, or shown again
    touch() {
      E_setHidden(deck, deck.slides[0], !deck.slides[0].hidden);
      commit();
    },
  })
);
shell.begin();
