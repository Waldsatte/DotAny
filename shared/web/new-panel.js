// New that first asks which kind of file: search above, kinds on the
// left, one click starts it. The shell makes one when a tool's newFile has groups; the tool says what each kind is and
// makes the file. The last five kinds chosen are kept, and {Mod+N} skips the panel for another of the last one.
import { el, root, reduce, sound, store, icon, EASE } from './shared.js';
import { T } from './i18n.js';
import { inkfall } from './inkfall-input.js';
import { used } from './keys.js';

/** groups: [{ key, label, items: [{ id, label, ext, also }] }]; create(item): make it; before(): put away what's open;
 *  after(): give the work its focus back when the panel closes without a choice. */
export function createNewPanel({ groups, create, before = () => {}, after = () => {}, key = 'dotany-new-recent' }) {
  const ALL = groups.flatMap((g) => g.items);
  const byId = (id) => ALL.find((f) => f.id === id);
  const recent = () => {
    try {
      return (JSON.parse(store.get(key)) || []).filter(byId);
    } catch {
      return [];
    }
  };
  const wrap = el('div', 'new-wrap');
  wrap.setAttribute('role', 'dialog');
  wrap.setAttribute('aria-modal', 'true');
  wrap.setAttribute('aria-label', '新建');
  const box = el('div', 'new');
  const search = el('label', 'new-search'),
    field = el('span', 'fi'),
    input = document.createElement('input');
  input.type = 'text';
  input.id = 'newSearch';
  input.placeholder = '搜索格式';
  input.spellcheck = false;
  input.autocomplete = 'off';
  field.appendChild(input);
  search.innerHTML = icon('search');
  search.appendChild(field);
  const body = el('div', 'new-body'),
    nav = el('nav', 'set-nav new-nav'),
    pill = el('i', 'set-pl'),
    list = el('div', 'new-list dyn');
  nav.appendChild(pill);
  const TABS = [{ key: 'recent', label: '最近使用' }, ...groups];
  let page = groups[0].key,
    rows = [],
    active = -1;
  const tabs = TABS.map((g) => {
    const b = el('button', 'set-tab', g.label);
    b.type = 'button';
    b.dataset.k = g.key;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      const from = TABS.findIndex((t) => t.key === page),
        to = TABS.findIndex((t) => t.key === g.key);
      page = g.key;
      if (input.value) {
        input.value = '';
        input.dispatchEvent(new Event('input'));
      }
      draw(Math.sign(to - from));
      input.focus();
    });
    nav.appendChild(b);
    return b;
  });
  body.append(nav, list);
  box.append(search, body);
  wrap.appendChild(box);
  document.body.appendChild(wrap);
  inkfall(input);
  function items() {
    const q = input.value.trim().toLowerCase();
    if (q)
      return ALL.filter((f) => (f.label + ' ' + f.ext + ' ' + f.id + ' ' + (f.also || '')).toLowerCase().includes(q));
    if (page === 'recent') return recent().map(byId);
    return groups.find((g) => g.key === page).items;
  }
  function placePill() {
    const searching = !!input.value.trim();
    tabs.forEach((b) => b.setAttribute('aria-current', String(!searching && b.dataset.k === page)));
    const on = tabs.find((b) => b.dataset.k === page);
    pill.style.opacity = searching ? 0 : 1;
    pill.style.transform = 'translate(' + on.offsetLeft + 'px, ' + on.offsetTop + 'px)';
    pill.style.width = on.offsetWidth + 'px';
    pill.style.height = on.offsetHeight + 'px';
  }
  function mark() {
    rows.forEach((b, i) => b.classList.toggle('on', i === active));
    if (rows[active]) rows[active].scrollIntoView({ block: 'nearest' });
  }
  function draw(dir) {
    const now = items(),
      searching = !!input.value.trim();
    list.textContent = '';
    rows = now.map((f) => {
      const b = el('button', 'new-row');
      b.type = 'button';
      b.append(el('span', 'new-name', T(f.label)), el('span', 'new-ext', f.ext));
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        choose(f);
      });
      list.appendChild(b);
      return b;
    });
    if (!now.length)
      list.appendChild(el('p', 'new-none', T(searching ? '未找到匹配的格式。' : '暂无最近使用的格式。')));
    // with a search, Enter takes the first result at once
    active = searching && now.length ? 0 : -1;
    mark();
    placePill();
    if (dir && !reduce)
      list.animate(
        [
          { opacity: 0, transform: 'translateY(' + 10 * dir + 'px)' },
          { opacity: 1, transform: 'none' },
        ],
        { duration: 240, easing: EASE }
      );
  }
  const isOpen = () => wrap.classList.contains('on');
  function open() {
    before();
    input.value = '';
    input.dispatchEvent(new Event('input'));
    page = recent().length ? 'recent' : groups[0].key;
    draw(0);
    wrap.classList.add('on');
    root.classList.add('new-open');
    setTimeout(() => {
      placePill();
      input.focus();
    }, 30);
  }
  function close() {
    if (!isOpen()) return false;
    wrap.classList.remove('on');
    root.classList.remove('new-open');
    input.blur();
    return true;
  }
  function choose(f) {
    close();
    store.set(key, JSON.stringify([f.id, ...recent().filter((id) => id !== f.id)].slice(0, 5)));
    create(f);
  }
  input.addEventListener('input', () => {
    if (isOpen()) draw(0);
  });
  wrap.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      used(e);
      sound();
      close();
      after();
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (rows.length) {
        active = (active + (e.key === 'ArrowDown' ? 1 : rows.length - 1 + (active < 0 ? 1 : 0))) % rows.length;
        mark();
      }
      return;
    }
    if (e.key === 'Enter' && document.activeElement === input) {
      e.preventDefault();
      if (rows[active]) rows[active].click();
      return;
    }
    if (e.key !== 'Tab') return;
    // the keyboard stays inside the panel
    const all = [...box.querySelectorAll('input, button')],
      at = all.indexOf(document.activeElement);
    e.preventDefault();
    all[(at + (e.shiftKey ? all.length - 1 : 1)) % all.length].focus();
  });
  wrap.addEventListener('pointerdown', (e) => {
    if (e.target === wrap) {
      close();
      after();
    }
  });
  return {
    wrap,
    open,
    close,
    isOpen,
    byId,
    /** {Mod+N}: another of the last kind chosen, without the panel */
    quick() {
      choose(byId(recent()[0]) || ALL[0]);
    },
  };
}
