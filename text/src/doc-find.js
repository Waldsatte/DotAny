// ---------- find and replace for documents: the same glass as code files, searching across a paragraph's formatting ----------
import { Plugin, PluginKey, TextSelection } from 'prosemirror-state';
import { Decoration, DecorationSet } from 'prosemirror-view';
import { inkfall as fieldInk } from '@dotany/web/inkfall-input.js';

const key = new PluginKey('docFind');

// every match, as document positions; a word may span several bold or coloured runs within one paragraph
function search(doc, query, matchCase) {
  const out = [];
  if (!query) return out;
  const needle = matchCase ? query : query.toLowerCase();
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    let text = '';
    const map = [];
    node.forEach((child, offset) => {
      if (!child.isText) return;
      for (let i = 0; i < child.text.length; i++) map.push(pos + 1 + offset + i);
      text += child.text;
    });
    const hay = matchCase ? text : text.toLowerCase();
    for (let at = hay.indexOf(needle); at >= 0; at = hay.indexOf(needle, at + needle.length)) {
      out.push({ from: map[at], to: map[at + needle.length - 1] + 1 });
    }
    return false;
  });
  return out;
}

export const docFindPlugin = new Plugin({
  key,
  state: {
    init: () => ({ query: '', matchCase: false, matches: [], cur: -1 }),
    apply(tr, st) {
      const m = tr.getMeta(key);
      let next = st;
      if (m) next = { ...st, ...m };
      if ((m && ('query' in m || 'matchCase' in m)) || (tr.docChanged && next.query)) {
        const matches = search(tr.doc, next.query, next.matchCase);
        next = {
          ...next,
          matches,
          cur: matches.length ? Math.min(next.cur < 0 ? 0 : next.cur, matches.length - 1) : -1,
        };
      }
      return next;
    },
  },
  props: {
    decorations(state) {
      const { matches, cur } = key.getState(state);
      if (!matches.length) return null;
      return DecorationSet.create(
        state.doc,
        matches.map((m, i) => Decoration.inline(m.from, m.to, { class: i === cur ? 'pm-match cur' : 'pm-match' }))
      );
    },
  },
});

export function createDocFind({ getView, el, ic, sound, parent, before, T, onClose, pressed }) {
  const box = el('div', 'cfind dfind');
  box.setAttribute('role', 'search');
  box.setAttribute('aria-label', '查找');
  const wrap = (id, ph) => {
    const w = el('span', 'fi'),
      i = document.createElement('input');
    i.type = 'text';
    i.id = id;
    i.placeholder = ph;
    i.spellcheck = false;
    i.autocomplete = 'off';
    w.appendChild(i);
    return { w, i };
  };
  const q = wrap('docFindText', '查找'),
    r = wrap('docReplaceText', '替换为');
  const count = el('span', 'f-count dyn');
  const btn = (name, label, run, toggle) => {
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
  const st = () => key.getState(getView().state);
  const setQuery = () => {
    const v = getView();
    v.dispatch(
      v.state.tr.setMeta(key, { query: q.i.value, matchCase: bCase.getAttribute('aria-pressed') === 'true', cur: 0 })
    );
    reveal();
    sync();
  };
  const reveal = () => {
    const v = getView(),
      { matches, cur } = st();
    if (cur < 0) return;
    v.dispatch(
      v.state.tr.setSelection(TextSelection.create(v.state.doc, matches[cur].from, matches[cur].to)).scrollIntoView()
    );
  };
  const step = (d) => {
    const v = getView(),
      { matches, cur } = st();
    if (!matches.length) return;
    v.dispatch(v.state.tr.setMeta(key, { cur: (cur + d + matches.length) % matches.length }));
    reveal();
    sync();
  };
  const sync = () => {
    const { matches, cur, query } = st();
    count.textContent = !query ? '' : matches.length ? `${cur + 1}/${matches.length}` : T('无匹配结果');
    box.classList.toggle('none', !!query && !matches.length);
  };
  const bMore = btn(
    'right',
    '替换',
    () => {
      box.classList.toggle('rep');
      bMore.setAttribute('aria-pressed', String(box.classList.contains('rep')));
      if (box.classList.contains('rep')) r.i.focus();
    },
    true
  );
  bMore.classList.add('f-more');
  const bCase = btn(
    'caseOn',
    '区分大小写',
    (b) => {
      b.setAttribute('aria-pressed', String(b.getAttribute('aria-pressed') !== 'true'));
      setQuery();
    },
    true
  );
  const bPrev = btn('up', '上一个', () => step(-1)),
    bNext = btn('down', '下一个', () => step(1));
  const bClose = btn('close', '关闭', () => close());
  const row1 = el('div', 'f-row'),
    row2 = el('div', 'f-row f-rep'),
    repBox = el('div', 'f-repbox');
  const one = el('button', 'btn soft f-t', '替换'),
    all = el('button', 'btn soft f-t', '全部替换');
  one.type = 'button';
  all.type = 'button';
  one.id = 'docReplaceOne';
  all.id = 'docReplaceAll';
  [one, all].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
  one.addEventListener('click', () => {
    sound();
    const v = getView(),
      { matches, cur } = st();
    if (cur < 0) return;
    v.dispatch(v.state.tr.insertText(r.i.value, matches[cur].from, matches[cur].to));
    reveal();
    sync();
  });
  all.addEventListener('click', () => {
    sound();
    const v = getView(),
      { matches } = st();
    if (!matches.length) return;
    const tr = v.state.tr;
    for (let i = matches.length - 1; i >= 0; i--) tr.insertText(r.i.value, matches[i].from, matches[i].to);
    v.dispatch(tr);
    sync();
  });
  row1.append(bMore, q.w, count, bCase, bPrev, bNext, bClose);
  row2.append(r.w, one, all);
  repBox.appendChild(row2);
  box.append(row1, repBox);
  parent.insertBefore(box, before);
  fieldInk(q.i);
  fieldInk(r.i);
  function open(replace) {
    const v = getView(),
      s = v.state.selection;
    const picked = s.empty ? '' : v.state.doc.textBetween(s.from, s.to, ' ');
    if (picked && picked.length < 80) q.i.value = picked;
    box.classList.add('on');
    pressed(true);
    if (replace) {
      box.classList.add('rep');
      bMore.setAttribute('aria-pressed', 'true');
    }
    setQuery();
    q.i.focus();
    q.i.select();
    return true;
  }
  function close() {
    if (!box.classList.contains('on')) return false;
    box.classList.remove('on');
    pressed(false);
    const v = getView();
    v.dispatch(v.state.tr.setMeta(key, { query: '', cur: -1 }));
    onClose();
    return true;
  }
  q.i.addEventListener('input', setQuery);
  r.i.addEventListener('input', () => {});
  [q.i, r.i].forEach((i) =>
    i.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        sound();
        close();
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        if (i === r.i) one.click();
        else step(e.shiftKey ? -1 : 1);
      }
    })
  );
  return { box, open, close, isOpen: () => box.classList.contains('on'), sync };
}
