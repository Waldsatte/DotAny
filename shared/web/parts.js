// The family's small parts, as DotAny Image and Audio build them: the dock (.pill) and its buttons, a slider (.slide),
// a switch row (.row.compact + .tog) and chips (.chip). Each takes what it shows and what to do; the tool keeps the state.
import { el, icon, sound } from './shared.js';
import { T } from './i18n.js';

/** A button for the dock: an icon with its name on hover. */
export function barBtn(id, n, tip, run) {
  const b = el('button', 'tb');
  b.type = 'button';
  b.id = id;
  b.innerHTML = icon(n);
  b.setAttribute('aria-label', tip);
  b.dataset.tip = tip;
  b.addEventListener('mousedown', (e) => e.preventDefault());
  b.addEventListener('click', run);
  return b;
}

/** The dock at the bottom: groups of buttons with a line between them. */
export function pill(...groups) {
  const p = el('div', 'pill');
  groups.forEach((g, i) => {
    if (i) p.appendChild(el('i', 'sep'));
    p.append(...g);
  });
  return p;
}

/**
 * A slider: its name and value above, both 12px. `get` reads the value; `input` sets it as it moves; `start` is called once
 * as a drag begins (where the tool remembers for undo). `center` fills from the middle. Returns the label; call `.sync()`
 * after the value changes elsewhere.
 */
export function slider({ id, label, min, max, step, get, input, fmt, center, start }) {
  const w = el('label', 'slide'),
    top = el('span', 'slide-top'),
    val = el('span');
  top.append(el('b', '', T(label)), val);
  const i = document.createElement('input');
  i.type = 'range';
  i.id = id;
  i.min = min;
  i.max = max;
  i.step = step;
  i.setAttribute('aria-label', T(label));
  const paintIt = () => {
    i.value = get();
    const p = ((i.value - min) / (max - min)) * 100,
      c = center ? 50 : 0;
    i.style.setProperty('--a', Math.min(p, c) + '%');
    i.style.setProperty('--b', Math.max(p, c) + '%');
    val.textContent = fmt(+i.value);
  };
  let started = false;
  i.addEventListener('input', () => {
    if (!started) {
      if (start) start();
      started = true;
    }
    input(+i.value);
    paintIt();
  });
  i.addEventListener('change', () => {
    started = false;
  });
  paintIt();
  w.append(top, i);
  w.sync = paintIt;
  return w;
}

/** A switch in a panel: `.row.compact` with `.tog`. */
export function toggleRow({ id, label, get, toggle }) {
  const r = el('button', 'row compact');
  r.type = 'button';
  r.id = id;
  r.setAttribute('role', 'switch');
  r.innerHTML = '<span class="row-t"><b></b></span><i class="tog"></i>';
  r.querySelector('b').textContent = T(label);
  r.addEventListener('mousedown', (e) => e.preventDefault());
  r.addEventListener('click', () => {
    sound();
    toggle();
    r.sync();
  });
  r.sync = () => r.setAttribute('aria-checked', String(!!get()));
  r.sync();
  return r;
}

/** Three or four choices in a row: `.chip`. `options` is [[value, Chinese label], …]. */
export function chips({ id, options, get, pick }) {
  const box = el('div', 'chips');
  if (id) box.id = id;
  const btns = options.map(([v, t]) => {
    const b = el('button', 'chip', T(t));
    b.type = 'button';
    b.dataset.v = v;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      pick(v);
      box.sync();
    });
    box.appendChild(b);
    return b;
  });
  box.sync = () => btns.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === get())));
  box.sync();
  return box;
}

/**
 * Settings › Language: one row, and the family's menu (.pop) from a button that names the language
 * on screen in its own words: System, then every language LANGS lists. openPop(button, items, pick) is the window's
 * own menu (shell/file-menu.js's, or the hub's); get() is what is stored (null for System), pick(v) stores it.
 */
export function languageRow({ LANGS, get, pick, openPop }) {
  const row = el('div', 'row lang-row');
  row.innerHTML = '<span class="row-t"><b>界面语言</b><small>所有 DotAny 工具一起切换。</small></span>';
  const btn = el('button', 'btn soft lang-pick');
  btn.type = 'button';
  btn.id = 'langPick';
  const now = el('span', 'dyn');
  btn.append(now);
  btn.insertAdjacentHTML('beforeend', icon('chevron'));
  btn.addEventListener('mousedown', (e) => e.preventDefault());
  btn.addEventListener('click', () => {
    sound();
    const v = get() || 'system';
    openPop(
      btn,
      [
        { value: 'system', label: '跟随系统', on: v === 'system', id: 'lang-system' },
        { sep: true },
        ...LANGS.filter((l) => !l.test).map((l) => ({
          value: l.code,
          label: l.name,
          raw: true,
          on: v === l.code,
          id: 'lang-' + l.code,
        })),
      ],
      (c) => pick(c === 'system' ? null : c)
    );
  });
  row.append(btn);
  const sync = () => {
    const v = get(),
      l = v && LANGS.find((x) => x.code === v);
    now.textContent = l ? l.name : T('跟随系统');
    btn.dataset.v = l ? l.code : 'system';
  };
  return { row, btn, sync };
}

/**
 * A chip whose words don't fit its column takes two (or more) of the row's columns, so it is never cut and the short ones
 * beside it (1:1, 4:3) stay as they are (the layout adapts to the sentence). For every grid of .chips
 * under root; the shell runs it when the language, the window's size or the fonts change, and when chips are drawn.
 */
export function fitChips(root = document) {
  for (const box of root.querySelectorAll('.chips')) {
    const cs = getComputedStyle(box);
    if (cs.display !== 'grid' || !box.offsetWidth) continue;
    const cols = cs.gridTemplateColumns.split(' ').length,
      gap = parseFloat(cs.columnGap) || 0;
    const colW = (box.clientWidth - gap * (cols - 1)) / cols;
    for (const chip of box.children) {
      if (!chip.classList.contains('chip')) continue;
      const r = document.createRange();
      r.selectNodeContents(chip);
      const need = r.getBoundingClientRect().width + 16;
      const span = need > colW ? Math.min(cols, Math.ceil((need + gap) / (colW + gap))) : 0;
      const want = span > 1 ? 'span ' + span : '';
      if (chip.style.gridColumn !== want) chip.style.gridColumn = want;
    }
  }
}
