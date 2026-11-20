// The Mac with more than one screen: the presenter's view in a window of its own, on the presenter's
// screen, while the show fills the other. This is that window's page: the same presenter's view the show
// draws in its own window on one screen (main.js `setPresenter`), its parts and classes, filled from what the show's page
// sends (`presenter-state`). It holds no deck: every key, button and click goes back to the show's page, which moves the
// show and sends the new state. The window belongs to its document window: it goes when the show ends or that window
// goes, and the show ends when it goes (closed, ⌘W, ⌘Q).
import { el, root, reduce, icon, sound, EASE, store, LUCIDE as ICONS } from '@dotany/web/shared.js';
import { on, closeWindow } from '@dotany/web/backend.js';
import { presenterDo } from './backend.js';

export function presenterWindow(lucide) {
  Object.assign(ICONS, lucide);
  const MOVE = { duration: 240, easing: EASE };
  const show = el('div', 'show on pview'),
    showMain = el('div', 'show-main'),
    showHold = el('div', 'show-hold'),
    endCard = el('div', 'show-end'),
    endWord = el('span', '', '放映结束');
  endCard.appendChild(endWord);
  showMain.append(showHold, endCard);
  const pvSide = el('div', 'pv-side'),
    pvTime = el('div', 'pv-time'),
    pvClock = el('span', 'pv-clock', '00:00'),
    pvNow = el('span', 'pv-now'),
    pvReset = el('button', 'btn soft', '重置计时器');
  pvReset.type = 'button';
  pvReset.id = 'pvReset';
  pvTime.append(pvClock, pvNow, el('i', 'pages-gap'), pvReset);
  const pvNextLab = el('span', 'side-label', '下一页'),
    pvNext = el('div', 'pv-next'),
    pvNotesLab = el('span', 'side-label', '演讲者备注'),
    pvNotes = el('div', 'pv-notes');
  pvSide.append(pvTime, pvNextLab, pvNext, pvNotesLab, pvNotes);
  const barBtn = (id, n, run) => {
    const b = el('button', 'tb');
    b.type = 'button';
    b.id = id;
    b.innerHTML = icon(n);
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => {
      sound();
      run();
    });
    return b;
  };
  const hudNo = el('span', 'hud-no', '1 / 1'),
    hudPrev = barBtn('hudPrev', 'prev', () => presenterDo('prev')),
    hudNext = barBtn('hudNext', 'next', () => presenterDo('next')),
    hudBlack = barBtn('hudBlack', 'black', () => presenterDo('black')),
    hudExit = barBtn('hudExit', 'close', () => presenterDo('end'));
  hudBlack.setAttribute('aria-pressed', 'false');
  const hud = el('div', 'pill hud'),
    hudRow = el('div', 'pill-row');
  hudRow.append(hudPrev, hudNo, hudNext, el('i', 'sep'), hudBlack, el('i', 'sep'), hudExit);
  hud.appendChild(hudRow);
  show.append(showMain, pvSide, hud);
  show.tabIndex = -1;
  document.body.appendChild(show);

  // the show's pictures, sent once each: the show's page's own addresses, and this page's for the same bytes
  const pics = new Map();
  let state = null,
    startedAt = Date.now();
  const local = (html) => {
    for (const [from, to] of pics) html = html.split(from).join(to);
    return html;
  };
  const slideOf = (html) => {
    const t = document.createElement('template');
    t.innerHTML = local(html);
    return t.content.firstElementChild;
  };
  const tip = (b, words) => {
    b.setAttribute('aria-label', words);
    b.dataset.tip = words;
  };
  function place() {
    if (!state) return;
    const s = showHold.firstElementChild;
    if (s) {
      const b = showMain.getBoundingClientRect(),
        k = Math.min(b.width / state.w, b.height / state.h);
      s.style.transformOrigin = '0 0';
      s.style.position = 'absolute';
      s.style.left = 0;
      s.style.top = 0;
      s.style.transform = `translate(${(b.width - state.w * k) / 2}px, ${(b.height - state.h * k) / 2}px) scale(${k})`;
    }
    pvNext.textContent = '';
    if (state.next) {
      const box = pvNext.getBoundingClientRect(),
        k = Math.min(box.width / state.w, box.height / state.h),
        n = slideOf(state.next),
        w = el('div', 'pv-paper');
      n.style.transformOrigin = '0 0';
      n.style.position = 'absolute';
      n.style.left = 0;
      n.style.top = 0;
      n.style.transform = `scale(${k})`;
      w.style.width = state.w * k + 'px';
      w.style.height = state.h * k + 'px';
      w.appendChild(n);
      pvNext.appendChild(w);
    }
  }
  async function take(next) {
    for (const [from, data] of Object.entries(next.pics || {}))
      if (!pics.has(from)) pics.set(from, URL.createObjectURL(await (await fetch(data)).blob()));
    const turned = !state || state.at !== next.at || state.next !== next.next;
    if (!state) startedAt = next.startedAt || Date.now();
    state = next;
    const w = next.words;
    if (next.theme) root.dataset.theme = next.theme;
    else delete root.dataset.theme;
    if (next.lang) root.lang = next.lang;
    if (next.sound != null) store.took('dotany-sound', next.sound);
    endWord.textContent = w.end;
    pvReset.textContent = w.reset;
    pvNotesLab.textContent = w.notes;
    pvNextLab.textContent = next.next ? w.next : w.end;
    tip(hudPrev, w.prev);
    tip(hudNext, w.next);
    tip(hudBlack, w.black);
    tip(hudExit, w.exit);
    hudNo.textContent = `${next.at} / ${next.of}`;
    hudBlack.setAttribute('aria-pressed', String(next.veil === 'black'));
    endCard.classList.toggle('on', !!next.ended);
    showHold.textContent = '';
    const s = slideOf(next.cur);
    if (s) showHold.appendChild(s);
    place();
    if (turned) {
      pvNotes.textContent = '';
      pvNotes.scrollTop = 0;
      if (next.notes)
        next.notes.split('\n').forEach((line) => {
          const q = el('p', '', line);
          q.dataset.file = '';
          pvNotes.appendChild(q);
        });
      else pvNotes.appendChild(el('p', 'pv-empty', w.empty));
      if (!reduce) {
        pvNotes.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
        if (pvNext.firstElementChild) pvNext.firstElementChild.animate([{ opacity: 0 }, { opacity: 1 }], MOVE);
      }
    }
  }
  // what the show's page sends comes in order, each after the last is drawn
  let queue = Promise.resolve();
  // once this page hears it, the show's page sends everything (`ready`)
  on('presenter-state', (next) => {
    queue = queue.then(() => take(next)).catch(() => {});
  }).then(() => presenterDo('ready'));
  addEventListener('resize', place);

  const tick = () => {
    const t = Math.floor((Date.now() - startedAt) / 1000),
      m = Math.floor(t / 60),
      h = Math.floor(m / 60),
      p = (n) => String(n).padStart(2, '0');
    pvClock.textContent = h ? `${h}:${p(m % 60)}:${p(t % 60)}` : `${p(m)}:${p(t % 60)}`;
    const d = new Date();
    pvNow.textContent = `${p(d.getHours())}:${p(d.getMinutes())}`;
  };
  tick();
  setInterval(tick, 1000);
  pvReset.addEventListener('click', () => {
    sound();
    startedAt = Date.now();
    tick();
  });

  // the show's keys, as in the show's own window; the ⌘ keys are the menu bar's (⌘W closes this window, and the show ends)
  addEventListener(
    'keydown',
    (e) => {
      if (e.metaKey || e.ctrlKey) return;
      e.preventDefault();
      presenterDo('key:' + e.key);
    },
    true
  );
  showMain.addEventListener('click', () => presenterDo('next'));
  // nothing to save here: the window closes when asked (the red button, ⌘W, ⌘Q)
  on('close-requested', () => closeWindow());
  show.focus();
}
