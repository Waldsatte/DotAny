// ---------- code and config files: double-click, change, leave. The same caret and glass as the rest of DotAny ----------
import { EditorState, Compartment, StateField, StateEffect, Annotation } from '@codemirror/state';
import {
  EditorView,
  ViewPlugin,
  Decoration,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  drawSelection,
  rectangularSelection,
  crosshairCursor,
  dropCursor,
} from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import {
  syntaxHighlighting,
  HighlightStyle,
  indentOnInput,
  bracketMatching,
  foldGutter,
  foldKeymap,
  syntaxTree,
  indentUnit,
} from '@codemirror/language';
import {
  search,
  searchKeymap,
  SearchQuery,
  setSearchQuery,
  getSearchQuery,
  findNext,
  findPrevious,
  replaceNext,
  replaceAll,
  openSearchPanel,
  closeSearchPanel,
  searchPanelOpen,
  highlightSelectionMatches,
} from '@codemirror/search';
import { linter, lintKeymap } from '@codemirror/lint';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { tags as t } from '@lezer/highlight';
import { FOLLOW, REST, FRESH, glideStep, farJump } from '@dotany/web/core.js';
import { inkfall as fieldInk } from '@dotany/web/inkfall-input.js';
import { num } from '@dotany/web/i18n.js';
import { LUCIDE } from './code-icons.js';
import { createLong } from './long-scroll.js';
import { long as longFile } from './backend.js';
import { modKey } from '@dotany/web/keys.js';

// each language arrives when a file first needs it
const LANGUAGES = {
  json: () => import('@codemirror/lang-json').then((m) => m.json()),
  python: () => import('@codemirror/lang-python').then((m) => m.python()),
  js: () => import('@codemirror/lang-javascript').then((m) => m.javascript()),
  jsx: () => import('@codemirror/lang-javascript').then((m) => m.javascript({ jsx: true })),
  ts: () => import('@codemirror/lang-javascript').then((m) => m.javascript({ typescript: true })),
  tsx: () => import('@codemirror/lang-javascript').then((m) => m.javascript({ typescript: true, jsx: true })),
  html: () => import('@codemirror/lang-html').then((m) => m.html()),
  css: () => import('@codemirror/lang-css').then((m) => m.css()),
  yaml: () => import('@codemirror/lang-yaml').then((m) => m.yaml()),
  xml: () => import('@codemirror/lang-xml').then((m) => m.xml()),
};

const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
const root = document.documentElement;

// ---------- one colour of ink: the structure of code is told by weight and tone, orange stays for the caret ----------
const inkStyle = HighlightStyle.define([
  { tag: [t.propertyName], color: 'var(--text)', fontWeight: '500' },
  {
    tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.definitionKeyword, t.moduleKeyword],
    color: 'var(--text)',
    fontWeight: '500',
  },
  {
    tag: [t.function(t.definition(t.variableName)), t.definition(t.className)],
    color: 'var(--text)',
    fontWeight: '500',
  },
  { tag: [t.tagName, t.className, t.typeName, t.heading], color: 'var(--text)', fontWeight: '500' },
  { tag: [t.string, t.special(t.string), t.attributeValue], color: 'var(--code-soft)' },
  { tag: [t.attributeName], color: 'var(--code-soft)' },
  { tag: [t.number, t.bool, t.null], color: 'var(--text)', fontStyle: 'italic' },
  { tag: [t.comment, t.lineComment, t.blockComment], color: 'var(--code-faint)', fontStyle: 'italic' },
  {
    tag: [t.punctuation, t.separator, t.bracket, t.brace, t.squareBracket, t.paren, t.operator],
    color: 'var(--code-faint)',
  },
]);

// ---------- the typing feel: the caret glides and new letters fade in, with the shared numbers ----------
const caretPlugin = ViewPlugin.fromClass(
  class {
    constructor(view) {
      this.view = view;
      this.dom = document.createElement('div');
      this.dom.className = 'cm-dcaret is-off';
      this.dom.setAttribute('aria-hidden', 'true');
      view.scrollDOM.appendChild(this.dom);
      this.cur = null;
      this.until = 0;
      this.raf = 0;
      this.last = 0;
      this.movedAt = 0;
      this.frame = this.frame.bind(this);
      this.kick();
    }
    update(u) {
      if (
        u.selectionSet
        || u.docChanged
        || u.geometryChanged
        || u.focusChanged
        || u.viewportChanged
        || u.transactions.some((tr) => tr.effects.some((e) => e.is(nudgeCaret)))
      )
        this.kick();
    }
    kick() {
      this.until = performance.now() + FOLLOW;
      if (!this.raf) {
        this.last = performance.now();
        this.raf = requestAnimationFrame(this.frame);
      }
    }
    target() {
      const v = this.view,
        r = v.state.selection.main;
      if (!r.empty || !v.hasFocus) return null;
      root.classList.add('measuring');
      const c = v.coordsAtPos(r.head, r.assoc || 1) || v.coordsAtPos(r.head, -1);
      root.classList.remove('measuring');
      if (!c) return null;
      const box = v.scrollDOM.getBoundingClientRect();
      return {
        x: c.left - box.left + v.scrollDOM.scrollLeft,
        y: c.top - box.top + v.scrollDOM.scrollTop,
        h: c.bottom - c.top,
      };
    }
    frame(now) {
      this.raf = 0;
      const dt = Math.min(64, now - this.last);
      this.last = now;
      const tg = this.target();
      if (!tg) {
        this.dom.classList.add('is-off');
        if (now < this.until) this.raf = requestAnimationFrame(this.frame);
        return;
      }
      let c = this.cur,
        moving = false;
      if (reduce || farJump(c, tg, tg.h) || this.dom.classList.contains('is-off')) c = { ...tg };
      else ({ pos: c, moving } = glideStep(c, tg, dt));
      if (!this.cur || moving || c.x !== this.cur.x || c.y !== this.cur.y) this.movedAt = now;
      this.cur = c;
      this.dom.classList.remove('is-off');
      this.dom.style.transform = `translate(${c.x}px, ${c.y}px)`;
      this.dom.style.height = c.h + 'px';
      this.dom.classList.toggle('is-blink', now - this.movedAt > REST);
      if (moving || now < this.until) this.raf = requestAnimationFrame(this.frame);
      else if (!this.dom.classList.contains('is-blink'))
        setTimeout(() => this.kick(), REST - (now - this.movedAt) + 20);
    }
    destroy() {
      cancelAnimationFrame(this.raf);
      this.dom.remove();
    }
  }
);
const dropFresh = StateEffect.define();
const quietChange = Annotation.define();
const nudgeCaret = StateEffect.define();
const freshField = StateField.define({
  create: () => [],
  update(list, tr) {
    let out = list;
    if (tr.docChanged && out.length)
      out = out
        .map((f) => ({ from: tr.changes.mapPos(f.from, 1), to: tr.changes.mapPos(f.to, -1), t: f.t }))
        .filter((f) => f.to > f.from);
    if (!reduce && tr.isUserEvent('input.type') && !tr.isUserEvent('input.type.compose')) {
      const now = performance.now();
      tr.changes.iterChanges((fa, ta, fb, tb, ins) => {
        if (tb > fb && !ins.toString().includes('\n')) out = [...out, { from: fb, to: tb, t: now }];
      });
    }
    for (const e of tr.effects) if (e.is(dropFresh)) out = out.filter((f) => e.value - f.t < FRESH);
    return out;
  },
});
const freshPlugin = ViewPlugin.fromClass(
  class {
    constructor(view) {
      this.view = view;
      this.decorations = Decoration.none;
      this.timer = 0;
    }
    update(u) {
      const list = u.state.field(freshField);
      if (!list.length || u.view.composing) {
        this.decorations = Decoration.none;
        return;
      }
      const now = performance.now();
      this.decorations = Decoration.set(
        list.map((f) =>
          Decoration.mark({
            class: 'inkfall-fresh',
            attributes: { style: `animation-delay:-${Math.round(now - f.t)}ms` },
          }).range(f.from, f.to)
        ),
        true
      );
      clearTimeout(this.timer);
      this.timer = setTimeout(() => this.view.dispatch({ effects: dropFresh.of(performance.now()) }), FRESH + 20);
    }
    destroy() {
      clearTimeout(this.timer);
    }
  },
  { decorations: (p) => p.decorations }
);

// the smallest change between two texts, so the caret and undo stay where they make sense
function diff(a, b) {
  let s = 0;
  const n = Math.min(a.length, b.length);
  while (s < n && a.charCodeAt(s) === b.charCodeAt(s)) s++;
  let e = 0;
  while (e < n - s && a.charCodeAt(a.length - 1 - e) === b.charCodeAt(b.length - 1 - e)) e++;
  return { from: s, to: a.length - e, insert: b.slice(s, b.length - e) };
}

const PUNCT = new Set(['[', ']', '{', '}', ',', ':', '⚠']);

export function createCode(ctx) {
  const { el, T, sound, openPop, closePop, parent, before } = ctx;
  const ic = (name) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${LUCIDE[name]}</svg>`;
  const host = el('div', 'codehost');
  parent.insertBefore(host, before);
  const wrapPart = new Compartment(),
    indentPart = new Compartment(),
    lockPart = new Compartment(),
    langPart = new Compartment();
  let view = null,
    kind = 'json',
    wrapOn = false,
    indentChoice = 2,
    eol = 'LF',
    encoding = 'UTF-8';
  const indentOf = { json: 2, python: 4, plain: 2, log: 2 };
  let findGen = 0,
    logFilter = 'all',
    followOn = false,
    locked = false,
    longTotal = 1,
    longLines = 1,
    findHit = null,
    findCount = null,
    logLabel = true;
  let langLabel = null;

  // ---------- the status line ----------
  const status = el('footer', 'cstatus');
  parent.insertBefore(status, before);
  const left = el('div', 'cs-l'),
    right = el('div', 'cs-r');
  const valid = el('button', 'cs-valid dyn');
  valid.type = 'button';
  const crumbs = el('div', 'cs-path dyn'),
    info = el('span', 'cs-info dyn');
  left.append(valid, info, crumbs);
  const stBtn = (id, name) => {
    const b = el('button', 'cs-b dyn');
    b.type = 'button';
    b.id = id;
    b.setAttribute('aria-label', name);
    return b;
  };
  const bPos = stBtn('csPos', '跳转至行'),
    bIndent = stBtn('csIndent', '缩进'),
    bEol = stBtn('csEol', '换行符'),
    bEnc = stBtn('csEnc', '编码');
  const bLang = el('span', 'cs-lang dyn');
  right.append(bPos, bIndent, bEol, bEnc, bLang);
  status.append(left, right);
  [bPos, bIndent, bEol, bEnc, valid].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));

  // ---------- reading the JSON tree ----------
  function pathAt(state, pos) {
    const out = [];
    for (let n = syntaxTree(state).resolveInner(pos, -1); n; n = n.parent) {
      if (n.name === 'Property') {
        const k = n.getChild('PropertyName');
        if (k) out.unshift({ label: state.sliceDoc(k.from + 1, k.to - 1), at: k.from });
      } else if (n.parent && n.parent.name === 'Array' && !PUNCT.has(n.name)) {
        let i = 0;
        for (let c = n.parent.firstChild; c && c.from < n.from; c = c.nextSibling) if (!PUNCT.has(c.name)) i++;
        out.unshift({ label: String(i), at: n.from });
      }
    }
    return out;
  }
  function outline(state) {
    const items = [];
    const summary = (v) => {
      if (!v) return '';
      if (v.name === 'Object') {
        let n = 0;
        for (let c = v.firstChild; c; c = c.nextSibling) if (c.name === 'Property') n++;
        return `{ ${n} }`;
      }
      if (v.name === 'Array') {
        let n = 0;
        for (let c = v.firstChild; c; c = c.nextSibling) if (!PUNCT.has(c.name)) n++;
        return `[ ${n} ]`;
      }
      const text = state.sliceDoc(v.from, v.to);
      return text.length > 22 ? text.slice(0, 21) + '…' : text;
    };
    const walk = (node, depth) => {
      if (node.name === 'Object') {
        for (let c = node.firstChild; c; c = c.nextSibling) {
          if (c.name !== 'Property') continue;
          const k = c.getChild('PropertyName');
          let v = k && k.nextSibling;
          while (v && PUNCT.has(v.name)) v = v.nextSibling;
          if (!k) continue;
          items.push({ key: state.sliceDoc(k.from + 1, k.to - 1), val: summary(v), from: k.from, end: c.to, depth });
          if (v) walk(v, depth + 1);
        }
      } else if (node.name === 'Array') {
        let i = 0;
        for (let c = node.firstChild; c; c = c.nextSibling) {
          if (PUNCT.has(c.name)) continue;
          items.push({ key: String(i++), val: summary(c), from: c.from, end: c.to, depth, index: true });
          walk(c, depth + 1);
        }
      }
    };
    const top = syntaxTree(state).topNode.firstChild;
    if (top) walk(top, 0);
    return items;
  }

  // ---------- the structure module: the shape of the file, beside it ----------
  const tree = el('aside', 'ctree');
  tree.setAttribute('aria-label', '大纲结构');
  const treeHead = el('div', 'ct-head', '大纲结构'),
    treeList = el('div', 'ct-list dyn');
  tree.append(treeHead, treeList);
  parent.insertBefore(tree, before);
  let treeOn = false,
    rows = [],
    treeTimer = 0;
  function drawTree() {
    if (!view || kind !== 'json') return;
    const items = outline(view.state),
      scroll = treeList.scrollTop;
    treeList.textContent = '';
    rows = items.map((it) => {
      const b = el('button', 'ct-row');
      b.type = 'button';
      b.style.paddingLeft = 8 + it.depth * 13 + 'px';
      b.append(el('span', it.index ? 'ct-k idx' : 'ct-k', it.key), el('span', 'ct-v', it.val));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        view.dispatch({ selection: { anchor: it.from }, effects: EditorView.scrollIntoView(it.from, { y: 'center' }) });
        view.focus();
      });
      treeList.appendChild(b);
      return { b, it };
    });
    treeList.scrollTop = scroll;
    markTree();
  }
  function markTree() {
    if (!view) return;
    const at = view.state.selection.main.head;
    let best = null;
    for (const r of rows) if (r.it.from <= at && at <= r.it.end) best = r;
    rows.forEach((r) => r.b.classList.toggle('on', r === best));
    if (best && treeOn) {
      const box = treeList.getBoundingClientRect(),
        b = best.b.getBoundingClientRect();
      if (b.top < box.top || b.bottom > box.bottom) treeList.scrollTop += b.top - box.top - box.height / 2;
    }
  }
  const setTree = (on) => {
    treeOn = on;
    root.classList.toggle('tree-on', on);
    ctx.buttons.tree.setAttribute('aria-pressed', String(on));
    if (on) drawTree();
  };

  // ---------- find and replace: floating glass, CodeMirror does the searching ----------
  const find = el('div', 'cfind');
  find.setAttribute('role', 'search');
  find.setAttribute('aria-label', '查找');
  const mkInput = (id, ph) => {
    const wrap = el('span', 'fi'),
      i = document.createElement('input');
    i.type = 'text';
    i.id = id;
    i.placeholder = ph;
    i.spellcheck = false;
    i.autocomplete = 'off';
    wrap.appendChild(i);
    return { wrap, i };
  };
  const fFind = mkInput('findText', '查找'),
    fRep = mkInput('replaceText', '替换为');
  const count = el('span', 'f-count dyn');
  const fBtn = (name, label, run, toggle) => {
    const b = el('button', 'f-b');
    b.type = 'button';
    b.innerHTML = ic(name);
    b.setAttribute('aria-label', label);
    b.dataset.tip = label;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      run(b);
    });
    if (toggle) b.setAttribute('aria-pressed', 'false');
    return b;
  };
  const toggleOpt = (b) => {
    b.setAttribute('aria-pressed', String(b.getAttribute('aria-pressed') !== 'true'));
    applyQuery();
  };
  const bCase = fBtn('caseOn', '区分大小写', toggleOpt, true),
    bWord = fBtn('word', '全字匹配', toggleOpt, true),
    bRe = fBtn('regex', '正则表达式', toggleOpt, true);
  const bPrev = fBtn('up', '上一个', () => findStep(false)),
    bNext = fBtn('down', '下一个', () => findStep(true));
  const bClose = fBtn('close', '关闭', () => closeFind());
  const bMore = fBtn(
    'right',
    '替换',
    () => {
      find.classList.toggle('rep');
      bMore.setAttribute('aria-pressed', String(find.classList.contains('rep')));
      if (find.classList.contains('rep')) fRep.i.focus();
    },
    true
  );
  bMore.classList.add('f-more');
  const row1 = el('div', 'f-row'),
    row2 = el('div', 'f-row f-rep');
  const repOne = el('button', 'btn soft f-t', '替换'),
    repAll = el('button', 'btn soft f-t', '全部替换');
  repOne.type = 'button';
  repAll.type = 'button';
  repOne.id = 'replaceOne';
  repAll.id = 'replaceAll';
  [repOne, repAll].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
  repOne.addEventListener('click', () => {
    sound();
    applyQuery();
    replaceNext(view);
  });
  repAll.addEventListener('click', () => {
    sound();
    applyQuery();
    replaceAll(view);
  });
  row1.append(bMore, fFind.wrap, count, bCase, bWord, bRe, bPrev, bNext, bClose);
  const repBox = el('div', 'f-repbox');
  row2.append(fRep.wrap, repOne, repAll);
  repBox.appendChild(row2);
  find.append(row1, repBox);
  parent.insertBefore(find, before);
  fieldInk(fFind.i);
  fieldInk(fRep.i);
  const on = (b) => b.getAttribute('aria-pressed') === 'true';
  function applyQuery() {
    if (kind === 'log') {
      findGen++;
      findHit = null;
      findCount = null;
      long.setCurrent(null);
      longFile.find(findGen, fFind.i.value, on(bCase), on(bWord), on(bRe));
      return;
    }
    if (!view) return;
    view.dispatch({
      effects: setSearchQuery.of(
        new SearchQuery({
          search: fFind.i.value,
          caseSensitive: on(bCase),
          wholeWord: on(bWord),
          regexp: on(bRe),
          replace: fRep.i.value,
        })
      ),
    });
    syncCount();
  }
  function syncCount() {
    if (kind === 'log') return;
    if (!view || !find.classList.contains('on')) return;
    const q = getSearchQuery(view.state);
    if (!q.search) {
      count.textContent = '';
      find.classList.remove('none');
      return;
    }
    if (!q.valid) {
      count.textContent = T('语法无效');
      find.classList.add('none');
      return;
    }
    const sel = view.state.selection.main;
    let n = 0,
      cur = 0;
    const c = q.getCursor(view.state);
    while (n < 5000) {
      const r = c.next();
      if (r.done) break;
      n++;
      if (r.value.from === sel.from && r.value.to === sel.to) cur = n;
    }
    count.textContent = n ? `${cur || '–'}/${n}` : T('无匹配结果');
    find.classList.toggle('none', !n);
  }
  // the long scroll searches on disk, in the background, and counts as it goes
  function showLongCount() {
    const s = findCount;
    if (!s || !fFind.i.value) {
      count.textContent = '';
      find.classList.remove('none');
      return;
    }
    if (!s.valid) {
      count.textContent = T('语法无效');
      find.classList.add('none');
      return;
    }
    const n = s.count.toLocaleString('en-US').replace(/,/g, '') + (s.capped || !s.done ? '+' : '');
    count.textContent = s.count || !s.done ? `${findHit ? findHit.index + 1 : '–'}/${n}` : T('无匹配结果');
    find.classList.toggle('none', s.done && !s.count);
  }
  async function stepLong(forward) {
    const g = findGen;
    const hit = await longFile.step(long.top(), forward, findHit ? findHit.index : null);
    // the search changed while waiting: this answer is about the old one
    if (!hit || g !== findGen) return;
    findHit = hit;
    long.setCurrent(hit);
    long.jump(hit.row, true);
    showLongCount();
  }
  const findStep = (forward) => (kind === 'log' ? stepLong(forward) : forward ? findNext(view) : findPrevious(view));
  function openFind() {
    sound();
    if (kind === 'log') {
      const picked = String(getSelection());
      if (picked && !picked.includes('\n')) fFind.i.value = picked;
      find.classList.add('on');
      ctx.buttons.find.setAttribute('aria-pressed', 'true');
      applyQuery();
      fFind.i.focus();
      fFind.i.select();
      return true;
    }
    const s = view.state,
      sel = s.selection.main;
    if (!sel.empty && !s.sliceDoc(sel.from, sel.to).includes('\n')) fFind.i.value = s.sliceDoc(sel.from, sel.to);
    if (!searchPanelOpen(s)) openSearchPanel(view);
    find.classList.add('on');
    ctx.buttons.find.setAttribute('aria-pressed', 'true');
    applyQuery();
    fFind.i.focus();
    fFind.i.select();
    return true;
  }
  function closeFind() {
    find.classList.remove('on');
    ctx.buttons.find.setAttribute('aria-pressed', 'false');
    if (kind === 'log') {
      fFind.i.value = '';
      applyQuery();
      long.focus();
      return;
    }
    if (view) {
      closeSearchPanel(view);
      view.focus();
    }
  }
  [fFind.i, fRep.i].forEach((i) => {
    i.addEventListener('input', applyQuery);
    i.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        sound();
        closeFind();
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        if (i === fRep.i) {
          applyQuery();
          replaceNext(view);
        } else findStep(!e.shiftKey);
        syncCount();
      }
    });
  });

  // ---------- jumping to a line ----------
  const go = el('div', 'cgoto');
  go.setAttribute('role', 'dialog');
  go.setAttribute('aria-label', '跳转至行');
  const goIn = mkInput('gotoLine', '行号');
  const goHint = el('span', 'go-hint dyn');
  go.append(goIn.wrap, goHint);
  parent.insertBefore(go, before);
  fieldInk(goIn.i);
  const closeGoto = (refocus = true) => {
    if (!go.classList.contains('on')) return;
    go.classList.remove('on');
    if (refocus) {
      if (kind === 'log') long.focus();
      else if (view) view.focus();
    }
  };
  function openGoto() {
    sound();
    const line = kind === 'log' ? long.top() + 1 : view.state.doc.lineAt(view.state.selection.main.head).number;
    const all = kind === 'log' ? long.total() : view.state.doc.lines;
    goIn.i.value = String(line);
    goHint.textContent = T('共 {n} 行', { n: all });
    go.classList.add('on');
    goIn.i.focus();
    goIn.i.select();
    return true;
  }
  goIn.i.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      sound();
      closeGoto();
      return;
    }
    if (e.key !== 'Enter') return;
    e.preventDefault();
    if (kind === 'log') {
      sound();
      long.jump((parseInt(goIn.i.value, 10) || 1) - 1, true);
      closeGoto();
      return;
    }
    const n = Math.max(1, Math.min(view.state.doc.lines, parseInt(goIn.i.value, 10) || 1)),
      l = view.state.doc.line(n);
    sound();
    view.dispatch({ selection: { anchor: l.from }, effects: EditorView.scrollIntoView(l.from, { y: 'center' }) });
    closeGoto();
  });
  document.addEventListener('pointerdown', (e) => {
    if (go.classList.contains('on') && !go.contains(e.target) && !bPos.contains(e.target)) closeGoto(false);
  });

  // ---------- formatting ----------
  const indentText = () => (indentChoice === 'tab' ? '\t' : ' '.repeat(indentChoice));
  function reflow(mode) {
    if (kind !== 'json') return false;
    const cur = view.state.doc.toString();
    let data;
    try {
      data = JSON.parse(cur);
    } catch {
      sound();
      firstError();
      return false;
    }
    const next = mode === 'min' ? JSON.stringify(data) : JSON.stringify(data, null, indentText()) + '\n';
    if (next !== cur) view.dispatch({ changes: diff(cur, next), userEvent: 'input.format', scrollIntoView: true });
    return true;
  }
  let problem = null;
  function firstError() {
    if (problem)
      view.dispatch({
        selection: { anchor: problem.from },
        effects: EditorView.scrollIntoView(problem.from, { y: 'center' }),
      });
    view.focus();
  }
  let parseLint = null;
  const jsonLint = (v) =>
    (parseLint ? parseLint(v) : []).map((d) => ({ ...d, message: T('此处存在 JSON 语法错误。') }));
  valid.addEventListener('click', () => {
    if (problem) {
      sound();
      firstError();
    }
  });

  // ---------- keeping the line below in step ----------
  let checkTimer = 0;
  function check() {
    if (!view) return;
    if (kind !== 'json' || !parseLint) {
      problem = null;
      valid.hidden = true;
      return;
    }
    valid.hidden = false;
    const d = parseLint(view)[0];
    problem = d || null;
    if (d) {
      const n = view.state.doc.lineAt(d.from).number;
      valid.className = 'cs-valid dyn bad';
      valid.textContent = T('第 {n} 行有语法错误', { n });
    } else {
      valid.className = 'cs-valid dyn';
      valid.textContent = T('JSON 语法有效');
    }
  }
  function syncLongStatus() {
    bPos.textContent = T('第 {n} 行', { n: long.top() + 1 });
    bLang.textContent = logLabel ? T('日志') : T('纯文本');
    info.textContent = T('只读 · 共 {n} 行', { n: num(longLines) });
    crumbs.textContent = '';
    valid.hidden = true;
  }
  function syncStatus() {
    if (kind === 'log') {
      syncLongStatus();
      return;
    }
    if (!view) return;
    const s = view.state,
      sel = s.selection.main,
      line = s.doc.lineAt(sel.head);
    bPos.textContent = T('第 {line} 行，第 {col} 列', { line: line.number, col: sel.head - line.from + 1 });
    bIndent.textContent = indentChoice === 'tab' ? 'Tab' : T('空格 {n}', { n: indentChoice });
    bEol.textContent = eol;
    bEnc.textContent = encoding;
    bLang.textContent = kind === 'json' ? 'JSON' : kind === 'python' ? 'Python' : langLabel || T('纯文本');
    info.textContent = '';
    crumbs.textContent = '';
    if (kind === 'json') {
      pathAt(s, sel.head).forEach((c, i) => {
        if (i) crumbs.appendChild(el('i', 'cs-sep', '›'));
        const b = el('button', 'cs-crumb', c.label);
        b.type = 'button';
        b.addEventListener('mousedown', (e) => e.preventDefault());
        b.addEventListener('click', () => {
          sound();
          view.dispatch({ selection: { anchor: c.at }, effects: EditorView.scrollIntoView(c.at, { y: 'center' }) });
          view.focus();
        });
        crumbs.appendChild(b);
      });
    }
  }

  // ---------- the editor ----------
  const foldIcon = (open) => {
    const s = el('span', 'cm-foldm' + (open ? ' open' : ''));
    s.innerHTML = ic('right');
    return s;
  };
  const lockFor = () => EditorState.readOnly.of(locked);
  const shared = () => [
    lockPart.of(lockFor()),
    langPart.of([]),
    lineNumbers(),
    foldGutter({ markerDOM: foldIcon }),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    history(),
    drawSelection({ cursorBlinkRate: 0 }),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    rectangularSelection(),
    crosshairCursor(),
    indentOnInput(),
    bracketMatching(),
    closeBrackets(),
    highlightSelectionMatches(),
    syntaxHighlighting(inkStyle),
    search({
      top: true,
      createPanel: () => {
        const dom = el('div');
        dom.style.display = 'none';
        return { dom, update: () => syncCount() };
      },
    }),
    wrapPart.of(wrapOn ? EditorView.lineWrapping : []),
    indentPart.of([indentUnit.of(indentText()), EditorState.tabSize.of(indentChoice === 'tab' ? 4 : indentChoice)]),
    caretPlugin,
    freshField,
    freshPlugin,
    EditorView.scrollMargins.of(() => ({ top: 72, bottom: 56, left: 56, right: 24 })),
    EditorView.contentAttributes.of({ spellcheck: 'false', autocapitalize: 'off', autocorrect: 'off' }),
    keymap.of([
      { key: 'Mod-f', run: openFind, preventDefault: true },
      {
        // on the Mac ⌘H hides the program; find and replace is ⌥⌘F there, as in TextEdit
        key: 'Mod-h',
        mac: 'Alt-Mod-f',
        run: () => {
          openFind();
          find.classList.add('rep');
          bMore.setAttribute('aria-pressed', 'true');
          return true;
        },
        preventDefault: true,
      },
      { key: 'Mod-g', run: openGoto, preventDefault: true },
      {
        key: 'Mod-s',
        run: () => {
          ctx.saveByHand();
          return true;
        },
        preventDefault: true,
      },
      {
        key: 'Mod-Shift-s',
        run: () => {
          ctx.openSaveAs();
          return true;
        },
        preventDefault: true,
      },
      {
        key: 'Shift-Alt-f',
        run: () => {
          sound();
          reflow('pretty');
          return true;
        },
        preventDefault: true,
      },
      {
        key: 'Alt-z',
        run: () => {
          sound();
          toggleWrap();
          return true;
        },
        preventDefault: true,
      },
      {
        key: 'Escape',
        run: () => {
          if (ctx.escape()) return true;
          if (find.classList.contains('on')) {
            sound();
            closeFind();
            return true;
          }
          return false;
        },
      },
      ...closeBracketsKeymap,
      ...searchKeymap,
      ...foldKeymap,
      ...lintKeymap,
      ...historyKeymap,
      indentWithTab,
      ...defaultKeymap,
    ]),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) {
        if (!u.transactions.every((tr) => tr.annotation(quietChange))) ctx.onEdit();
        clearTimeout(checkTimer);
        checkTimer = setTimeout(check, 160);
        clearTimeout(treeTimer);
        treeTimer = setTimeout(() => {
          if (treeOn) drawTree();
        }, 160);
        syncCount();
      }
      if (u.docChanged || u.selectionSet) {
        syncStatus();
        markTree();
        syncCount();
      }
    }),
  ];
  function applyLogFilter(v) {
    logFilter = v;
    long.setFilter(v);
    if (find.classList.contains('on')) applyQuery();
    ctx.buttons.filter.setAttribute('aria-pressed', String(v !== 'all'));
  }
  function setFollow(on) {
    followOn = on;
    ctx.buttons.follow.setAttribute('aria-pressed', String(on));
    if (kind === 'log') long.setFollow(on);
  }
  const makeState = (text) => EditorState.create({ doc: text, extensions: shared() });
  // the language comes a moment later, so opening a file never waits for it
  async function setLanguage(id) {
    if (!id || !LANGUAGES[id]) return;
    const wanted = view.state;
    const support = await LANGUAGES[id]();
    if (id === 'json') {
      const { jsonParseLinter } = await import('@codemirror/lang-json');
      parseLint = parseLint || jsonParseLinter();
    }
    if (view.state.doc !== wanted.doc && kind === 'log') return;
    view.dispatch({
      effects: langPart.reconfigure(id === 'json' ? [support, linter(jsonLint, { delay: 200 })] : support),
      annotations: quietChange.of(true),
    });
    check();
    syncStatus();
    if (treeOn) drawTree();
  }

  function toggleWrap() {
    wrapOn = !wrapOn;
    if (kind === 'log') long.setWrap(wrapOn);
    else view.dispatch({ effects: wrapPart.reconfigure(wrapOn ? EditorView.lineWrapping : []) });
    ctx.buttons.wrap.setAttribute('aria-pressed', String(wrapOn));
  }
  function setIndent(v) {
    indentChoice = v;
    indentOf[kind] = v;
    view.dispatch({
      effects: indentPart.reconfigure([indentUnit.of(indentText()), EditorState.tabSize.of(v === 'tab' ? 4 : v)]),
    });
    if (kind === 'json') {
      try {
        JSON.parse(view.state.doc.toString());
        reflow('pretty');
      } catch {
        /* an invalid file keeps its shape */
      }
    }
    syncStatus();
  }

  bPos.addEventListener('click', () => (go.classList.contains('on') ? closeGoto() : openGoto()));
  const menu = (b, items, cur, pick) => {
    sound();
    openPop(b, items, cur, pick);
  };
  bIndent.addEventListener('click', () =>
    menu(
      bIndent,
      [
        { value: 2, label: '2 个空格' },
        { value: 4, label: '4 个空格' },
        { value: 'tab', label: 'Tab' },
      ],
      indentChoice,
      setIndent
    )
  );
  bEol.addEventListener('click', () =>
    locked
      ? null
      : menu(
          bEol,
          [
            { value: 'LF', label: 'LF' },
            { value: 'CRLF', label: 'CRLF' },
          ],
          eol,
          (v) => {
            eol = v;
            ctx.pickEol(v);
            syncStatus();
          }
        )
  );
  // choosing here changes what the next save writes; the file's own encoding always appears in the list
  bEnc.addEventListener('click', () =>
    locked
      ? null
      : menu(
          bEnc,
          [...new Set(['UTF-8', 'UTF-8 BOM', 'GBK', encoding])].map((v) => ({ value: v, label: v })),
          encoding,
          (v) => {
            encoding = v;
            ctx.pickEncoding(v);
            syncStatus();
          }
        )
  );

  const long = createLong({
    el,
    host,
    onMove: () => {
      if (kind === 'log') syncLongStatus();
    },
    onCount: (p) => {
      longLines = p.lines;
      if (kind === 'log') syncLongStatus();
    },
    onFindCount: (s) => {
      // an answer for a search typed over since (answers from two searches can arrive in either order)
      if (s.seq !== findGen) return;
      findCount = s;
      showLongCount();
      // the first match shows itself as soon as it is found
      if (s.count && !findHit) stepLong(true);
    },
  });
  long.dom.hidden = true;
  const closeLong = () => {
    if (kind === 'log') {
      kind = 'plain';
      root.classList.remove('log-file');
      long.close();
      if (view) view.dom.hidden = false;
    }
  };
  long.dom.addEventListener('keydown', (e) => {
    const mod = modKey(e);
    const key = e.key.toLowerCase();
    if (mod && !e.shiftKey && key === 'f') {
      e.preventDefault();
      openFind();
    } else if (mod && !e.shiftKey && key === 'g') {
      e.preventDefault();
      openGoto();
    } else if (mod && e.shiftKey && key === 's') {
      e.preventDefault();
      ctx.openSaveAs();
    } else if (mod && !e.shiftKey && key === 's') e.preventDefault();
    else if (e.altKey && key === 'z') {
      e.preventDefault();
      sound();
      toggleWrap();
    } else if (e.key === 'Escape') {
      if (ctx.escape()) return;
      if (find.classList.contains('on')) {
        sound();
        closeFind();
      }
    }
  });

  return {
    host,
    status,
    find,
    tree,
    go,
    translatable: [status, find, tree, go],
    get kind() {
      return kind;
    },
    open(k, text, langId) {
      closeLong();
      closePop();
      if (find.classList.contains('on')) {
        find.classList.remove('on');
        ctx.buttons.find.setAttribute('aria-pressed', 'false');
      }
      kind = k;
      indentChoice = indentOf[k];
      const state = makeState(text);
      if (!view) view = new EditorView({ state, parent: host });
      else view.setState(state);
      view.dom.hidden = false;
      view.dispatch({
        effects: [
          wrapPart.reconfigure(wrapOn ? EditorView.lineWrapping : []),
          indentPart.reconfigure([
            indentUnit.of(indentText()),
            EditorState.tabSize.of(indentChoice === 'tab' ? 4 : indentChoice),
          ]),
          lockPart.reconfigure(lockFor()),
        ],
        annotations: quietChange.of(true),
      });
      root.classList.toggle('json-file', k === 'json');
      root.classList.remove('log-file');
      if (k !== 'json' && treeOn) setTree(false);
      check();
      syncStatus();
      if (treeOn) drawTree();
      setLanguage(langId);
      this.focus();
    },
    // the long scroll takes the same place, with the same status line and find bar
    async openLong({ logLabel: isLog }) {
      closePop();
      if (find.classList.contains('on')) {
        find.classList.remove('on');
        ctx.buttons.find.setAttribute('aria-pressed', 'false');
      }
      kind = 'log';
      logLabel = isLog;
      logFilter = 'all';
      findHit = null;
      findCount = null;
      longLines = 1;
      ctx.buttons.filter.setAttribute('aria-pressed', 'false');
      ctx.buttons.follow.setAttribute('aria-pressed', 'false');
      followOn = false;
      if (view) view.dom.hidden = true;
      root.classList.add('log-file');
      root.classList.remove('json-file');
      if (treeOn) setTree(false);
      long.setWrap(wrapOn);
      await long.open(false);
      syncStatus();
      long.focus();
    },
    refresh() {
      if (kind === 'log') {
        syncStatus();
        return;
      }
      if (!view) return;
      check();
      syncStatus();
      syncCount();
    },
    focus() {
      if (kind === 'log') long.focus();
      else if (view) view.focus();
    },
    actions: {
      find: () => (find.classList.contains('on') ? closeFind() : openFind()),
      formatMenu: (b) =>
        menu(
          b,
          [
            { value: 'pretty', label: '格式化', hint: '{Shift+Alt+F}' },
            { value: 'min', label: '压缩为单行' },
          ],
          '',
          (v) => reflow(v)
        ),
      wrap: () => toggleWrap(),
      tree: () => setTree(!treeOn),
      follow: () => setFollow(!followOn),
      filterMenu: (b) =>
        menu(
          b,
          [
            { value: 'all', label: '全部级别' },
            { value: 'warn', label: '警告与错误' },
            { value: 'error', label: '仅看错误' },
          ],
          logFilter,
          applyLogFilter
        ),
    },
    nudge() {
      if (view) view.dispatch({ effects: nudgeCaret.of(null) });
    },
    closeTransient() {
      closeGoto(false);
    },
    setEncoding(v) {
      encoding = v;
    },
    setEol(v) {
      eol = v;
    },
    setLang(v) {
      langLabel = v;
    },
    setLocked(on) {
      locked = on;
      if (view) view.dispatch({ effects: lockPart.reconfigure(lockFor()), annotations: quietChange.of(true) });
    },
    leave() {
      if (followOn) setFollow(false);
      if (find.classList.contains('on')) {
        if (kind !== 'log' && !view) return;
        closeFind();
      }
      if (treeOn) setTree(false);
      closeLong();
    },
    text: () => (view ? view.state.doc.toString() : ''),
  };
}
