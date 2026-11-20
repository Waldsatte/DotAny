// The article view: ProseMirror with the drawn caret, the bars over a selection and a table, and A4 sheets for Word.
import { DOMParser as PMParser, Fragment, Slice } from 'prosemirror-model';
import { EditorState, Plugin, PluginKey, TextSelection } from 'prosemirror-state';
import { EditorView, Decoration, DecorationSet } from 'prosemirror-view';
import { baseKeymap, toggleMark, chainCommands } from 'prosemirror-commands';
import { history, undo, redo } from 'prosemirror-history';
import { keymap } from 'prosemirror-keymap';
import { wrapInList, liftListItem, splitListItem, sinkListItem } from 'prosemirror-schema-list';
import {
  tableEditing,
  addRowAfter,
  addColumnAfter,
  deleteRow,
  deleteColumn,
  deleteTable,
  isInTable,
  goToNextCell,
} from 'prosemirror-tables';
import { FOLLOW, REST, FRESH, glideStep, farJump } from '@dotany/web/core.js';
import { app, el, root, reduce, sound } from '@dotany/web/shared.js';
import { T } from './i18n.js';
import { ICON } from './icons.js';
import { schema, DEFAULT_SIZE, DEFAULT_LH } from './schema.js';
import { docFindPlugin } from './doc-find.js';
import { linkRanges } from './markdown.js';
import {
  chrome,
  bar,
  seg,
  pill,
  pop,
  stage,
  scroller,
  editorHost,
  sels,
  lastNo,
  countEl,
  pageBtn,
  appEl,
  makeBtn,
  openPop,
  closePop,
  popOpenFor,
} from './shell.js';

const M = schema.marks,
  N = schema.nodes;
// each kind of document allows only what its format can keep
const RICH = ['doc', 'md'],
  WORD = ['doc'];
const only = (kinds, cmd) => (s, d, v) => (kinds.includes(app.kind) ? cmd(s, d, v) : true);
export let view;
// ---------- commands ----------
const eachBlock = (state, fn) => {
  const { from, to } = state.selection;
  state.doc.nodesBetween(from, to, (n, pos, parent, index) => {
    if (n.isTextblock) {
      fn(n, pos, parent, index);
      return false;
    }
    return true;
  });
};
const blockKind = (state) => {
  const p = state.selection.$from.parent;
  return p.type === N.heading ? 'h' + p.attrs.level : p.type === N.paragraph ? 'p' : '';
};
const setBlock = (kind) => (view) => {
  const { state } = view,
    tr = state.tr,
    target = blockKind(state) === kind ? 'p' : kind;
  eachBlock(state, (n, pos, parent, index) => {
    const type = target === 'p' ? N.paragraph : N.heading;
    if (!parent.canReplaceWith(index, index + 1, type)) return;
    const base = { align: n.attrs.align, lh: n.attrs.lh };
    tr.setNodeMarkup(pos, type, target === 'p' ? base : { ...base, level: +target[1] });
  });
  if (tr.docChanged) view.dispatch(tr);
  return true;
};
const setBlockAttr = (key, value) => (view) => {
  const { state } = view,
    tr = state.tr;
  eachBlock(state, (n, pos) => tr.setNodeMarkup(pos, null, { ...n.attrs, [key]: value }));
  if (tr.docChanged) view.dispatch(tr);
  return true;
};
const markActive = (state, type) => {
  const { from, $from, to, empty } = state.selection;
  if (empty) return !!type.isInSet(state.storedMarks || $from.marks());
  return state.doc.rangeHasMark(from, to, type);
};
const markValue = (state, type, attr) => {
  const { $from, empty, from } = state.selection;
  const marks = empty
    ? state.storedMarks || $from.marks()
    : state.doc.resolve(from + 1 <= state.doc.content.size ? from + 1 : from).marks();
  const m = type.isInSet(marks);
  return m ? m.attrs[attr] : null;
};
const setValueMark = (type, attrs) => (view) => {
  const { state } = view,
    { from, to, empty } = state.selection;
  const tr = state.tr;
  if (empty) {
    attrs ? tr.addStoredMark(type.create(attrs)) : tr.removeStoredMark(type);
  } else {
    tr.removeMark(from, to, type);
    if (attrs) tr.addMark(from, to, type.create(attrs));
  }
  view.dispatch(tr);
  return true;
};
const inList = (state, type) => {
  const { $from } = state.selection;
  for (let d = $from.depth; d > 0; d--) if ($from.node(d).type === type) return true;
  return false;
};
const toggleList = (type) => (view) => {
  const { state, dispatch } = view;
  if (inList(state, type)) return liftListItem(N.list_item)(state, dispatch);
  if (inList(state, type === N.bullet_list ? N.ordered_list : N.bullet_list)) {
    let done = false;
    const { $from } = state.selection;
    for (let d = $from.depth; d > 0 && !done; d--) {
      const n = $from.node(d);
      if (n.type === N.bullet_list || n.type === N.ordered_list) {
        dispatch(state.tr.setNodeMarkup($from.before(d), type));
        done = true;
      }
    }
    return done;
  }
  return wrapInList(type)(state, dispatch);
};
export function insertTable(view) {
  const cell = (head, t) =>
    (head ? N.table_header : N.table_cell).create(null, N.paragraph.create(null, t ? schema.text(t) : null));
  const heads = ['事项', '详情', '附注'].map(T);
  const rows = [
    N.table_row.create(
      null,
      heads.map((h) => cell(true, h))
    ),
    N.table_row.create(
      null,
      heads.map(() => cell(false))
    ),
    N.table_row.create(
      null,
      heads.map(() => cell(false))
    ),
  ];
  const tr = view.state.tr.replaceSelectionWith(N.table.create(null, rows));
  const after = tr.mapping.map(view.state.selection.to);
  if (after >= tr.doc.content.size - 1 && tr.doc.lastChild.type !== N.paragraph)
    tr.insert(tr.doc.content.size, N.paragraph.create());
  view.dispatch(tr.scrollIntoView());
}
function insertImage(view) {
  const input = el('input');
  input.type = 'file';
  input.accept = 'image/*';
  input.addEventListener('change', () => {
    const f = input.files && input.files[0];
    if (!f) return;
    const r = new FileReader();
    r.onload = () => {
      const pic = new Image();
      pic.onload = () => {
        let src = String(r.result);
        // Word takes these as they are; anything else becomes a PNG
        if (!/^data:image\/(png|jpeg|gif)/.test(src)) {
          const c = el('canvas');
          c.width = pic.naturalWidth;
          c.height = pic.naturalHeight;
          c.getContext('2d').drawImage(pic, 0, 0);
          src = c.toDataURL('image/png');
        }
        const image = N.image.create({ src, alt: f.name, w: pic.naturalWidth, h: pic.naturalHeight });
        // a cell holds words, so a picture asked for inside a table goes right after it
        const { $from } = view.state.selection;
        let depth = $from.depth;
        while (depth > 0 && $from.node(depth).type !== N.table) depth--;
        const tr =
          depth > 0 ? view.state.tr.insert($from.after(depth), image) : view.state.tr.replaceSelectionWith(image);
        view.dispatch(tr.scrollIntoView());
        view.focus();
      };
      pic.src = String(r.result);
    };
    r.readAsDataURL(f);
  });
  input.click();
}

// ---------- letters settling in ----------
const freshKey = new PluginKey('fresh');
let composing = false,
  compStart = -1,
  freshTimer = 0;
const freshDrop = (view) => {
  clearTimeout(freshTimer);
  freshTimer = setTimeout(
    () => view.dispatch(view.state.tr.setMeta(freshKey, { drop: performance.now() })),
    FRESH + 20
  );
};
const freshPlugin = new Plugin({
  key: freshKey,
  state: {
    init: () => [],
    apply(tr, list) {
      let out = tr.docChanged
        ? list
            .map((f) => ({ from: tr.mapping.map(f.from, 1), to: tr.mapping.map(f.to, -1), t: f.t }))
            .filter((f) => f.to > f.from)
        : list;
      const m = tr.getMeta(freshKey);
      if (m && m.add) out = [...out, m.add];
      if (m && m.drop) out = out.filter((f) => m.drop - f.t < FRESH);
      return out;
    },
  },
  props: {
    // the age goes in as a negative delay, so a line redrawn mid-way does not start the fade again
    decorations(state) {
      const list = freshKey.getState(state);
      if (!list.length || composing) return null;
      const now = performance.now();
      return DecorationSet.create(
        state.doc,
        list.map((f) =>
          Decoration.inline(f.from, f.to, { class: 'fresh', style: `animation-delay:-${Math.round(now - f.t)}ms` })
        )
      );
    },
    handleTextInput(view, from, to, text) {
      if (composing || view.composing || reduce) return false;
      const tr = view.state.tr.insertText(text, from, to);
      // the caret follows the letter, even if a click's new place has not reached the editor's state yet
      tr.setSelection(TextSelection.create(tr.doc, from + text.length));
      tr.setMeta(freshKey, { add: { from, to: from + text.length, t: performance.now() } });
      view.dispatch(tr.scrollIntoView());
      freshDrop(view);
      return true;
    },
    // what an input method commits settles in as one piece
    handleDOMEvents: {
      compositionstart(view) {
        composing = true;
        compStart = view.state.selection.from;
        return false;
      },
      compositionend(view) {
        composing = false;
        const from = compStart;
        compStart = -1;
        if (from < 0 || reduce) return false;
        setTimeout(() => {
          const to = view.state.selection.head;
          if (to > from) {
            view.dispatch(view.state.tr.setMeta(freshKey, { add: { from, to, t: performance.now() } }));
            freshDrop(view);
          }
        }, 0);
        return false;
      },
    },
  },
});

// ---------- Markdown links stay the text they are; they only read as links ----------
const linksKey = new PluginKey('mdLinks');
const linkDecorations = (doc) => {
  const list = [];
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    const text = node.textContent;
    if (text.includes('[') || text.includes('http'))
      for (const [a, b] of linkRanges(text))
        list.push(Decoration.inline(pos + 1 + a, pos + 1 + b, { class: 'md-link' }));
    return false;
  });
  return DecorationSet.create(doc, list);
};
const linksPlugin = new Plugin({
  key: linksKey,
  state: {
    init: (_, s) => (s.doc ? linkDecorations(s.doc) : DecorationSet.empty),
    apply: (tr, set) => (tr.docChanged ? linkDecorations(tr.doc) : set),
  },
  props: { decorations: (s) => (app.kind === 'md' ? linksKey.getState(s) : null) },
});

// ---------- page view: a gap opens between blocks at each A4 sheet's end ----------
const pageKey = new PluginKey('pages');
const SHEET = 1123,
  MARGIN_Y = 96,
  GAP = 28,
  BODY = SHEET - MARGIN_Y * 2;
const breakWidget = (b) =>
  Decoration.widget(
    b.pos,
    () => {
      const d = el('div', 'pb'),
        gap = el('div', 'pb-gap'),
        no = el('div', 'pb-no', String(b.page));
      d.style.height = b.height + 'px';
      // a sheet's edge is the sheet's, even when the words here are indented
      gap.style.left = -(77 + b.indent) + 'px';
      no.style.left = -b.indent + 'px';
      d.contentEditable = 'false';
      d.append(no, gap);
      return d;
    },
    { side: -1, key: `pb-${b.page}-${b.pos}-${b.height}-${b.indent}`, ignoreSelection: true }
  );
const lockedIds = (doc) => {
  const ids = [];
  doc.forEach((n) => {
    if (n.type === N.doc_locked) ids.push(n.attrs.lid);
  });
  return ids;
};
const keepLocked = new Plugin({
  filterTransaction(tr, state) {
    if (!tr.docChanged || app.switching || app.kind !== 'doc') return true;
    const before = lockedIds(state.doc);
    if (!before.length) return true;
    const after = new Set(lockedIds(tr.doc));
    const fine = before.every((id) => after.has(id));
    if (!fine) sound();
    return fine;
  },
});
const pagePlugin = new Plugin({
  key: pageKey,
  state: {
    init: () => DecorationSet.empty,
    apply(tr, set) {
      const m = tr.getMeta(pageKey);
      if (m) return m.breaks.length ? DecorationSet.create(tr.doc, m.breaks.map(breakWidget)) : DecorationSet.empty;
      return tr.docChanged ? set.map(tr.mapping, tr.doc) : set;
    },
  },
  props: { decorations: (s) => pageKey.getState(s) },
});

const SIZES = [13, 15, 17, 20, 24, 32];
const COLORS = [
  { value: '', label: '墨黑（默认）', swatch: 'var(--text)' },
  { value: 'gray', label: '石墨灰', swatch: 'var(--c-gray)' },
  { value: 'red', label: '朱砂红', swatch: 'var(--c-red)' },
  { value: 'blue', label: '群青蓝', swatch: 'var(--c-blue)' },
  { value: 'green', label: '森林绿', swatch: 'var(--c-green)' },
];
const LHS = [1.2, 1.5, 1.7, 2, 2.5];
const ALIGN_ICONS = { left: ICON.left, center: ICON.center, right: ICON.right, justify: ICON.justify };
const ALIGNS = [
  { value: 'left', label: '左对齐', icon: ALIGN_ICONS.left },
  { value: 'center', label: '居中对齐', icon: ALIGN_ICONS.center },
  { value: 'right', label: '右对齐', icon: ALIGN_ICONS.right },
  { value: 'justify', label: '两端对齐', icon: ALIGN_ICONS.justify },
];
const BOLD = ICON.bold;
const ITALIC = ICON.italic;
const UNDER = ICON.underline;
const mark = (type) => () => toggleMark(type)(view.state, view.dispatch);
const curLh = () => view.state.selection.$from.parent.attrs.lh || DEFAULT_LH;
const curAlign = () => view.state.selection.$from.parent.attrs.align || 'left';
const curColor = () => markValue(view.state, M.color, 'c') || '';
const curSize = () =>
  markValue(view.state, M.size, 'px') || { h1: 40, h2: 26, h3: 20 }[blockKind(view.state)] || DEFAULT_SIZE;
const openColor = (b) => openPop(b, COLORS, curColor(), (v) => setValueMark(M.color, v ? { c: v } : null)(view));

const segs = [
  ['p', '正文'],
  ['h1', 'H1'],
  ['h2', 'H2'],
  ['h3', 'H3'],
].map(([k, t]) => {
  const b = makeBtn('sg', t, { '正文': '正文段落', H1: '一级标题', H2: '二级标题', H3: '三级标题' }[t], null, () =>
    setBlock(k)(view)
  );
  b.dataset.k = k;
  seg.appendChild(b);
  return b;
});
const bBold = makeBtn('tb', BOLD, '加粗', '加粗 · {Mod+B}', mark(M.strong));
const bItalic = makeBtn('tb', ITALIC, '斜体', '斜体 · {Mod+I}', mark(M.em));
const bUnder = makeBtn('tb', UNDER, '下划线', '下划线 · {Mod+U}', mark(M.underline));
const bSize = makeBtn('tb', '<span>17</span>', '字号大小', '字号', (b) =>
  openPop(
    b,
    SIZES.map((s) => ({ value: s, label: String(s) })),
    curSize(),
    (v) => setValueMark(M.size, v === DEFAULT_SIZE ? null : { px: v })(view)
  )
);
const bColor = makeBtn('tb', ICON.color, '文字色彩', '文字色彩', openColor);
const bAlign = makeBtn('tb', ALIGN_ICONS.left, '对齐方式', '对齐方式', (b) =>
  openPop(b, ALIGNS, curAlign(), (v) => setBlockAttr('align', v === 'left' ? null : v)(view))
);
const bLh = makeBtn('tb', ICON.lineHeight + '<span>1.7</span>', '段落行距', '行距', (b) =>
  openPop(
    b,
    LHS.map((v) => ({ value: v, label: v.toFixed(1) })),
    curLh(),
    (v) => setBlockAttr('lh', v === DEFAULT_LH ? null : v)(view)
  )
);
const bUl = makeBtn('tb', ICON.list, '无序列表', '无序列表', () => toggleList(N.bullet_list)(view));
const bOl = makeBtn('tb', ICON.ordered, '有序列表', '有序列表', () => toggleList(N.ordered_list)(view));
const bImg = makeBtn('tb', ICON.image, '插入图片', '插入图片', () => insertImage(view));
const bTable = makeBtn('tb', ICON.table, '插入表格', '插入表格', () => insertTable(view));
const group = (items) => {
  const g = el('div', 'grp');
  g.append(...items);
  return g;
};
bar.append(
  seg,
  el('i', 'sep'),
  group([bBold, bItalic, bUnder, bSize, bColor]),
  el('i', 'sep'),
  group([bAlign, bLh]),
  el('i', 'sep'),
  group([bUl, bOl]),
  el('i', 'sep'),
  group([bImg, bTable])
);
[bUnder, bSize, bColor].forEach((b) => b.classList.add('word-only'));
[...bar.children].slice(3, 5).forEach((b) => b.classList.add('word-only'));

// ---------- what the bar lights up ----------
let placed = '';
function syncBar() {
  const s = view.state,
    kind = blockKind(s);
  segs.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.k === kind)));
  const on = segs.find((b) => b.dataset.k === kind);
  const at = on ? on.offsetLeft + ',' + on.offsetWidth : '';
  if (at !== placed) {
    placed = at;
    pill.style.opacity = on ? 1 : 0;
    if (on) {
      pill.style.transform = `translateX(${on.offsetLeft}px)`;
      pill.style.width = on.offsetWidth + 'px';
    }
  }
  bBold.setAttribute('aria-pressed', String(markActive(s, M.strong)));
  bItalic.setAttribute('aria-pressed', String(markActive(s, M.em)));
  bUnder.setAttribute('aria-pressed', String(markActive(s, M.underline)));
  bSize.lastChild.textContent = Math.round(curSize() * 10) / 10;
  const c = curColor();
  bColor.querySelector('.bar').style.stroke = c ? `var(--c-${c})` : '';
  bAlign.innerHTML = ALIGN_ICONS[curAlign()];
  bLh.lastChild.textContent = (+curLh()).toFixed(1);
  bUl.setAttribute('aria-pressed', String(inList(s, N.bullet_list)));
  bOl.setAttribute('aria-pressed', String(inList(s, N.ordered_list)));
}
const countWords = (t) => {
  const cjk = (t.match(/[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/gu) || []).length;
  const latin = (
    t.replace(/[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/gu, ' ').match(/[\p{L}\p{N}]+/gu)
    || []
  ).length;
  return cjk + latin;
};
let pages = 1;
function syncCount() {
  const n = countWords(view.state.doc.textBetween(0, view.state.doc.content.size, ' ', ' '));
  const paged = root.classList.contains('paged');
  countEl.textContent = paged ? T('{n} 字 · 共 {pages} 页', { n, pages }) : T('{n} 字', { n });
}

// ---------- the drawn caret and selection, plus the small bars ----------
class Motion {
  constructor(v) {
    this.view = v;
    this.dom = el('div', 'caret is-off');
    this.dom.setAttribute('aria-hidden', 'true');
    this.bub = el('div', 'bub');
    this.bub.setAttribute('role', 'toolbar');
    this.bub.setAttribute('aria-label', '浮动格式栏');
    this.bBold = makeBtn('tb', BOLD, '加粗', null, mark(M.strong));
    this.bItalic = makeBtn('tb', ITALIC, '斜体', null, mark(M.em));
    this.bUnder = makeBtn('tb', UNDER, '下划线', null, mark(M.underline));
    this.bColor = makeBtn('tb', ICON.color, '文字色彩', null, openColor);
    this.bUnder.classList.add('word-only');
    this.bColor.classList.add('word-only');
    this.bub.append(this.bBold, this.bItalic, this.bUnder, this.bColor);
    this.tbl = el('div', 'bub tblbar');
    this.tbl.setAttribute('role', 'toolbar');
    this.tbl.setAttribute('aria-label', '表格工具栏');
    const tc = (cmd) => () => cmd(view.state, view.dispatch);
    [
      ['插入行', addRowAfter],
      ['插入列', addColumnAfter],
      ['删除当前行', deleteRow],
      ['删除当前列', deleteColumn],
      ['删除表格', deleteTable],
    ].forEach(([t, c]) => {
      this.tbl.appendChild(makeBtn('tb', t, t, null, tc(c)));
    });
    stage.append(this.dom, this.bub, this.tbl);
    this.cur = null;
    this.until = 0;
    this.raf = 0;
    this.last = 0;
    this.movedAt = 0;
    this.drag = false;
    this.shown = false;
    this.timer = 0;
    this.tblOn = false;
    this.frame = this.frame.bind(this);
    this.down = (e) => {
      if (e.button === 0 && v.dom.contains(e.target)) {
        this.drag = true;
        this.hideBub();
      }
    };
    this.up = () => {
      if (this.drag) {
        this.drag = false;
        this.later(10);
      }
    };
    document.addEventListener('pointerdown', this.down);
    document.addEventListener('pointerup', this.up);
    this.kick(true);
  }
  update(v, prev) {
    const docChanged = !prev.doc.eq(v.state.doc),
      selChanged = !prev.selection.eq(v.state.selection);
    if (docChanged || selChanged) {
      this.kick(false);
      this.drawSelection();
      syncBar();
      if (docChanged) {
        syncCount();
        app.noteEdit();
        schedulePages();
      }
      this.placeTable();
      if (docChanged && selChanged && v.state.selection.empty) this.hideBub();
      this.later(this.shown ? 0 : 150);
    }
  }
  kick(snap, length = FOLLOW) {
    if (snap) this.cur = null;
    this.until = Math.max(this.until, performance.now() + length);
    if (!this.raf) {
      this.last = performance.now();
      this.raf = requestAnimationFrame(this.frame);
    }
  }
  target() {
    const v = this.view,
      s = v.state.selection;
    if (!s.empty || !(s instanceof TextSelection) || !v.hasFocus()) return null;
    // a letter that is still rising must not carry the caret with it
    root.classList.add('measuring');
    let c;
    try {
      c = v.coordsAtPos(s.head, 1);
    } catch {
      c = null;
    }
    root.classList.remove('measuring');
    if (!c) return null;
    const box = stage.getBoundingClientRect();
    return { x: c.left - box.left, y: c.top - box.top, h: c.bottom - c.top };
  }
  frame(now) {
    this.raf = 0;
    const dt = Math.min(64, now - this.last);
    this.last = now;
    const t = this.target();
    if (!t) {
      this.dom.classList.add('is-off');
      if (now < this.until) this.raf = requestAnimationFrame(this.frame);
      return;
    }
    let c = this.cur,
      moving = false;
    // far jumps land at once; the glide is for writing and stepping
    if (reduce || farJump(c, t, t.h) || this.dom.classList.contains('is-off')) c = { ...t };
    else ({ pos: c, moving } = glideStep(c, t, dt));
    if (!this.cur || moving || c.x !== this.cur.x || c.y !== this.cur.y) this.movedAt = now;
    this.cur = c;
    this.dom.classList.remove('is-off');
    this.dom.style.transform = `translate(${c.x}px, ${c.y}px)`;
    this.dom.style.height = c.h + 'px';
    // it blinks only once it has been still a while
    this.dom.classList.toggle('is-blink', now - this.movedAt > REST);
    if (moving || now < this.until) this.raf = requestAnimationFrame(this.frame);
    else if (!this.dom.classList.contains('is-blink'))
      setTimeout(() => this.kick(false), REST - (now - this.movedAt) + 20);
  }
  // the selection's rectangles, line by line; existing ones are reused so they stretch
  drawSelection() {
    const v = this.view,
      s = v.state.selection,
      rects = [];
    if (!s.empty && s instanceof TextSelection) {
      const box = stage.getBoundingClientRect();
      v.state.doc.nodesBetween(s.from, s.to, (n, pos) => {
        if (!n.isText) return true;
        const from = Math.max(s.from, pos),
          to = Math.min(s.to, pos + n.nodeSize);
        if (to <= from) return false;
        const a = v.domAtPos(from),
          b = v.domAtPos(to),
          r = document.createRange();
        try {
          r.setStart(a.node, a.offset);
          r.setEnd(b.node, b.offset);
        } catch {
          return false;
        }
        root.classList.add('measuring');
        for (const q of r.getClientRects())
          if (q.width > 0.5) rects.push({ l: q.left - box.left, t: q.top - box.top, w: q.width, h: q.height });
        root.classList.remove('measuring');
        return false;
      });
    }
    const lines = [];
    for (const r of rects) {
      const line = lines.find((m) => r.t < m.t + m.h - 2 && r.t + r.h > m.t + 2);
      if (line) {
        const right = Math.max(line.l + line.w, r.l + r.w),
          bottom = Math.max(line.t + line.h, r.t + r.h);
        line.l = Math.min(line.l, r.l);
        line.t = Math.min(line.t, r.t);
        line.w = right - line.l;
        line.h = bottom - line.t;
      } else lines.push({ ...r });
    }
    while (sels.children.length > lines.length) sels.lastChild.remove();
    while (sels.children.length < lines.length) sels.appendChild(el('i'));
    lines.forEach((m, i) => {
      const d = sels.children[i].style;
      d.left = m.l + 'px';
      d.top = m.t + 'px';
      d.width = m.w + 'px';
      d.height = m.h + 'px';
    });
  }
  later(ms) {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.placeBub(), ms);
  }
  placeBub() {
    const v = this.view,
      s = v.state.selection;
    if (this.drag || s.empty || !(s instanceof TextSelection) || !v.hasFocus()) {
      if (!popOpenFor()) this.hideBub();
      return;
    }
    let a, b;
    try {
      a = v.coordsAtPos(s.from, 1);
      b = v.coordsAtPos(Math.min(s.to, s.$from.end()), -1);
    } catch {
      this.hideBub();
      return;
    }
    this.bBold.setAttribute('aria-pressed', String(markActive(v.state, M.strong)));
    this.bItalic.setAttribute('aria-pressed', String(markActive(v.state, M.em)));
    this.bUnder.setAttribute('aria-pressed', String(markActive(v.state, M.underline)));
    const c = curColor();
    this.bColor.querySelector('.bar').style.stroke = c ? `var(--c-${c})` : '';
    const box = stage.getBoundingClientRect(),
      w = this.bub.offsetWidth,
      h = this.bub.offsetHeight;
    // over several lines it stands at the selection's start, where the eye already is
    const mid = Math.abs(a.top - b.top) < 4 ? (a.left + b.right) / 2 : a.left + w / 2;
    let x = Math.max(8, Math.min(stage.clientWidth - w - 8, mid - box.left - w / 2));
    let y = a.top - box.top - h - 10;
    const below = a.top - chrome.getBoundingClientRect().bottom < h + 16;
    this.bub.classList.toggle('below', below);
    if (below) y = b.bottom - box.top + 10;
    // it rises in where it first appears, then follows the selection
    if (!this.shown) this.bub.classList.add('jump');
    this.bub.style.setProperty('--x', x + 'px');
    this.bub.style.setProperty('--y', y + 'px');
    if (!this.shown) {
      void this.bub.offsetWidth;
      this.bub.classList.remove('jump');
      this.shown = true;
      requestAnimationFrame(() => this.bub.classList.add('on'));
    }
  }
  hideBub() {
    clearTimeout(this.timer);
    if (!this.shown) return false;
    this.shown = false;
    this.bub.classList.remove('on');
    return true;
  }
  placeTable() {
    const v = this.view,
      on = isInTable(v.state) && v.hasFocus();
    if (!on) {
      if (this.tblOn) {
        this.tblOn = false;
        this.tbl.classList.remove('on');
      }
      return;
    }
    const { $from } = v.state.selection;
    let d = $from.depth;
    while (d > 0 && $from.node(d).type !== N.table) d--;
    if (!d) return;
    const dom = v.nodeDOM($from.before(d));
    if (!dom) return;
    const box = stage.getBoundingClientRect(),
      r = dom.getBoundingClientRect();
    if (!this.tblOn) this.tbl.classList.add('jump');
    this.tbl.style.setProperty('--x', r.right - box.left - this.tbl.offsetWidth + 'px');
    this.tbl.style.setProperty('--y', r.top - box.top - this.tbl.offsetHeight - 8 + 'px');
    if (!this.tblOn) {
      void this.tbl.offsetWidth;
      this.tbl.classList.remove('jump');
      this.tblOn = true;
      requestAnimationFrame(() => this.tbl.classList.add('on'));
    }
  }
  relayout(length) {
    this.kick(false, length);
    this.drawSelection();
    this.placeTable();
    this.later(0);
  }
  // the bars go with the view; the next view makes its own
  destroy() {
    cancelAnimationFrame(this.raf);
    this.dom.remove();
    this.bub.remove();
    this.tbl.remove();
  }
}
let motion,
  wordBars = null;
// each opened document makes its own bars; settings words them as they are made, in the language on screen
export const onBars = (f) => {
  wordBars = f;
};
const motionPlugin = new Plugin({
  view(v) {
    motion = new Motion(v);
    if (wordBars) wordBars(motion);
    return motion;
  },
  props: {
    handleDOMEvents: {
      focus() {
        if (motion) {
          motion.kick(false);
          motion.placeTable();
          motion.later(0);
        }
        return false;
      },
      blur() {
        if (motion) {
          motion.kick(false);
          setTimeout(() => motion.placeTable(), 0);
        }
        return false;
      },
    },
  },
});

// ---------- computing the sheets ----------
let pageTimer = 0;
function schedulePages() {
  if (!root.classList.contains('paged')) return;
  clearTimeout(pageTimer);
  pageTimer = setTimeout(computePages, 80);
}
function computePages() {
  const { state } = view,
    list = [];
  let pageStart = null,
    page = 1,
    lastBottom = 0;
  const base = view.dom.getBoundingClientRect();
  // positions as if no gap were open, so the answer does not depend on the gaps already there
  const open = [...view.dom.querySelectorAll('.pb')].map((w) => ({
    top: w.getBoundingClientRect().top,
    h: parseFloat(w.style.height) || 0,
  }));
  const natural = (y) => {
    let shift = 0;
    for (const w of open) if (w.top < y - 0.5) shift += w.h;
    return y - base.top - shift;
  };
  const gapAt = (pos, atTop, left) => {
    const rest = BODY - (atTop - pageStart);
    list.push({
      pos,
      page,
      indent: Math.round(left - base.left),
      height: Math.round(rest + MARGIN_Y + GAP + MARGIN_Y),
    });
    page++;
    pageStart = atTop;
  };
  // the lines of a block of words, as they lie on screen
  const linesOf = (dom) => {
    const r = document.createRange();
    r.selectNodeContents(dom);
    const lines = [];
    for (const q of r.getClientRects()) {
      if (q.width <= 0) continue;
      const line = lines.find((l) => q.top < l.bottom - 2 && q.bottom > l.top + 2);
      if (line) {
        line.top = Math.min(line.top, q.top);
        line.bottom = Math.max(line.bottom, q.bottom);
        line.left = Math.min(line.left, q.left);
      } else lines.push({ top: q.top, bottom: q.bottom, left: q.left });
    }
    return lines.sort((x, y) => x.top - y.top);
  };
  const place = (node, pos, dom, atomic) => {
    const r = dom.getBoundingClientRect();
    const top = natural(r.top),
      bottom = natural(r.bottom);
    if (pageStart == null) pageStart = top;
    if (bottom - pageStart > BODY) {
      if (atomic) {
        if (top > pageStart) gapAt(pos, top, r.left);
      } else {
        // words fill the sheet to its last line that fits, and go on at the top of the next
        for (const line of linesOf(dom)) {
          const lineTop = natural(line.top),
            lineBottom = natural(line.bottom);
          if (lineBottom - pageStart <= BODY) continue;
          if (lineTop <= pageStart) continue;
          const found = view.posAtCoords({ left: line.left + 1, top: line.top + 1 });
          const at = found ? Math.max(pos + 1, Math.min(pos + node.content.size + 1, found.pos)) : pos + 1;
          gapAt(at, lineTop, r.left);
        }
      }
    }
    lastBottom = bottom + (parseFloat(getComputedStyle(dom).marginBottom) || 0);
  };
  state.doc.descendants((node, pos, parent) => {
    if (node.isTextblock) {
      const dom = view.nodeDOM(pos);
      if (dom && dom.getBoundingClientRect) place(node, pos, dom, false);
      return false;
    }
    // a table and a picture stay whole on their sheet
    if (node.type === N.table || node.isAtom) {
      const dom = view.nodeDOM(pos);
      if (dom && dom.getBoundingClientRect)
        place(node, pos, (dom.closest && dom.closest('.tableWrapper')) || dom, true);
      return false;
    }
    return true;
  });
  pages = page;
  stage.style.setProperty('--foot', Math.max(MARGIN_Y, BODY - (lastBottom - (pageStart || 0)) + MARGIN_Y) + 'px');
  lastNo.textContent = String(page);
  view.dispatch(view.state.tr.setMeta(pageKey, { breaks: list }).setMeta('addToHistory', false));
  syncCount();
  if (motion) motion.relayout(FOLLOW);
}
let leaving = 0;
function setPaged(on) {
  clearTimeout(leaving);
  pageBtn.setAttribute('aria-pressed', String(on));
  if (on) {
    root.classList.remove('leaving');
    root.classList.add('paged');
    computePages();
    if (motion) motion.relayout(560);
  } else {
    root.classList.add('leaving');
    if (motion) motion.relayout(560);
    leaving = setTimeout(
      () => {
        root.classList.remove('leaving', 'paged');
        view.dispatch(view.state.tr.setMeta(pageKey, { breaks: [] }).setMeta('addToHistory', false));
        syncCount();
        if (motion) motion.relayout(560);
      },
      reduce ? 0 : 200
    );
  }
  syncCount();
}
pageBtn.addEventListener('mousedown', (e) => e.preventDefault());
pageBtn.addEventListener('click', () => {
  sound();
  setPaged(pageBtn.getAttribute('aria-pressed') !== 'true');
  view.focus();
});

// ---------- the editor ----------
const listEnter = splitListItem(N.list_item);
view = new EditorView(editorHost, {
  // what a paste can bring in is limited to what the open file can keep
  handlePaste: (v, e) => pasteAsText(v, e),
  transformPasted: keepWhatTheFormatCan,
  attributes: { 'aria-label': T('正文编辑区'), spellcheck: 'false' },
  // the glass covers the top of the scroller, so the caret must stop short of it
  scrollMargin: { top: 141, bottom: 80, left: 0, right: 0 },
  scrollThreshold: { top: 141, bottom: 80, left: 0, right: 0 },
  state: EditorState.create({
    schema,
    plugins: [
      history(),
      keymap({
        'Mod-b': only(RICH, toggleMark(M.strong)),
        'Mod-i': only(RICH, toggleMark(M.em)),
        'Mod-u': only(WORD, toggleMark(M.underline)),
        'Mod-z': undo,
        'Mod-y': redo,
        'Mod-Shift-z': redo,
        'Mod-s': () => app.saveByHand(),
        'Mod-f': () => app.docFind.open(false),
        'Mod-h': () => app.docFind.open(true),
        'Mod-Shift-s': () => {
          app.openSaveAs();
          return true;
        },
        'Mod-Alt-0': only(RICH, (s, d, v) => setBlock('p')(v)),
        'Mod-Alt-1': only(RICH, (s, d, v) => setBlock('h1')(v)),
        'Mod-Alt-2': only(RICH, (s, d, v) => setBlock('h2')(v)),
        'Mod-Alt-3': only(RICH, (s, d, v) => setBlock('h3')(v)),
        Enter: listEnter,
        'Shift-Enter': only(WORD, (s, dispatch) => {
          dispatch(s.tr.replaceSelectionWith(N.hard_break.create()).scrollIntoView());
          return true;
        }),
        Tab: chainCommands(goToNextCell(1), sinkListItem(N.list_item), () => true),
        'Shift-Tab': chainCommands(goToNextCell(-1), liftListItem(N.list_item), () => true),
        Escape: () => {
          if (app.docFind && app.docFind.close()) return true;
          if (app.closeSettings()) return true;
          if (popOpenFor()) {
            closePop();
            return true;
          }
          return !!(motion && motion.hideBub());
        },
      }),
      keymap(baseKeymap),
      keepLocked,
      tableEditing(),
      pagePlugin,
      docFindPlugin,
      linksPlugin,
      freshPlugin,
      motionPlugin,
    ],
  }),
});

export { motion, syncBar, syncCount, setPaged, computePages, schedulePages };
export const resetBar = () => {
  placed = '';
  syncBar();
};
const STRIP_FOR_MD = ['underline', 'size', 'color', 'rx'];
function cleanForMarkdown(fragment) {
  const out = [];
  fragment.forEach((n) => {
    if (n.type === N.hard_break) {
      out.push(schema.text(' '));
      return;
    }
    if (n.isText) {
      out.push(n.mark(n.marks.filter((m) => !STRIP_FOR_MD.includes(m.type.name))));
      return;
    }
    const attrs = 'align' in n.attrs ? { ...n.attrs, align: null, lh: null } : n.attrs;
    out.push(n.type.create(attrs, cleanForMarkdown(n.content), n.marks));
  });
  return Fragment.fromArray(out);
}
function withoutIds(fragment) {
  const out = [];
  fragment.forEach((n) => {
    if (n.isText || (n.isLeaf && n.type !== N.image)) {
      if (n.type !== N.doc_locked) out.push(n.mark(n.marks.filter((m) => m.type !== M.rx)));
      return;
    }
    if (n.type === N.image) {
      out.push(n.type.create({ ...n.attrs, iid: null }, null, n.marks));
      return;
    }
    const attrs = { ...n.attrs };
    for (const k of ['pid', 'tid']) if (k in attrs) attrs[k] = null;
    out.push(n.type.create(attrs, withoutIds(n.content), n.marks));
  });
  return Fragment.fromArray(out);
}
function keepWhatTheFormatCan(slice) {
  // nothing pasted brings a place in the file with it
  if (app.kind === 'doc') return new Slice(withoutIds(slice.content), slice.openStart, slice.openEnd);
  return app.kind === 'md' ? new Slice(cleanForMarkdown(slice.content), slice.openStart, slice.openEnd) : slice;
}
// plain text takes only words, one paragraph per line
function pasteAsText(v, e) {
  if (app.kind !== 'txt' || !e.clipboardData) return false;
  const text = e.clipboardData.getData('text/plain').replace(/\r\n?/g, '\n');
  const lines = text.split('\n').map((l) => N.paragraph.create(null, l ? schema.text(l) : null));
  v.dispatch(v.state.tr.replaceSelection(new Slice(Fragment.fromArray(lines), 1, 1)).scrollIntoView());
  return true;
}
// a new file or a reloaded one starts with a clean undo history; the editor's plugins stay
export function showDoc(doc) {
  app.switching = true;
  const next = EditorState.create({ doc, plugins: view.state.plugins });
  view.updateState(next.apply(next.tr.setSelection(TextSelection.atStart(doc))));
  app.switching = false;
  scroller.scrollTop = 0;
  placed = '';
  syncBar();
  syncCount();
  if (motion) motion.relayout(FOLLOW);
}
