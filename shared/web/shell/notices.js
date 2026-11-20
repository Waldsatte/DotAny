// Notices from outside: the family's one row under the top bar, the tool's own kinds in it, and rows of
// the tool's own under it.
// cfg read here:
// - failWords: more of what a failed save can say, by its kind
// - keptMine(): Keep Current Edits marks the file unsaved, in every tool; a tool that keeps its own unsaved state does
//   it here.
// - noticeKinds: { kind: { icon, text(), actions } } — notices of the tool's own in the family's one row, shown with
//   shell.showNotice(kind); actions may be a function, and a run(button) that answers true opens a menu from its button
//   and keeps the row (Text's Encoding); noticeKinds may be a function, looked up when shown.
// - noticeMoved(on) is told when the row comes or goes, so the work can move under it.
// shell.notice(text, buttons, opts), below, is a row of the tool's own; a button given
// { menu: true } opens a menu from itself and its row stays.
import * as be from '../backend.js';
import { el, sound, icon } from '../shared.js';
import { T } from '../i18n.js';
import { later } from './later.js';

/** Takes from ctx: cfg, chrome, saveAsBtn, focusWork, the file's state, openSaveAs; calls openPath; reads ctx.doc and
 *  reads and writes ctx.lastSave. Gives the functions below, and ctx.noticeKind, ctx.failKind and ctx.kept. */
export function notices(ctx) {
  const { cfg, chrome, saveAsBtn, focusWork, changed, setState, openSaveAs } = ctx;
  const { openPath } = later(ctx);
  // ---------- notices from outside: the family's one row ----------
  const notice = el('div', 'notice'),
    noticeIn = el('div', 'notice-in');
  noticeIn.setAttribute('role', 'status');
  const nIcon = el('span', 'notice-i'),
    nText = el('p', 'notice-t'),
    nBtns = el('div', 'notice-b');
  noticeIn.append(nIcon, nText, nBtns);
  notice.appendChild(noticeIn);
  chrome.appendChild(notice);
  let noticeKind = '',
    failKind = 'other';
  const FAIL = {
    readonly: '文件为只读状态，未能保存。当前编辑仍完整保留在此窗口中。',
    locked: '文件正被其他程序占用，未能保存。当前编辑仍完整保留在此窗口中。',
    diskfull: '磁盘空间不足，未能保存。当前编辑仍完整保留在此窗口中。',
    denied: '缺少写入权限，未能保存。当前编辑仍完整保留在此窗口中。',
    ...(cfg.failWords || {}),
  };
  const failText = () => T(FAIL[failKind] || '未能完成保存。当前编辑仍完整保留在此窗口中。');
  // Keep Current Edits: what's on disk is now someone else's, so what's on screen is unsaved, and closing asks
  let kept = false;
  function keepMine() {
    be.keepMine();
    if (cfg.keptMine) cfg.keptMine();
    else if (ctx.doc) {
      kept = true;
      changed();
    }
  }
  const NOTICES = {
    changed: {
      icon: 'info',
      text: () => T('该文件已在外部被修改。'),
      actions: [
        [
          '载入外部修改',
          () => {
            loadOutside();
          },
        ],
        ['保留当前编辑', keepMine],
      ],
    },
    deleted: { icon: 'alert', text: () => T('该文件已在外部被移动或删除。'), actions: [['保留当前编辑', keepMine]] },
    savefail: {
      icon: 'alert',
      text: failText,
      actions: [
        [
          '另存为…',
          () => {
            openSaveAs(saveAsBtn, ctx.lastSave && ctx.lastSave.after);
          },
        ],
        ['知道了', null],
      ],
    },
  };
  // the tool's own kinds, looked up when shown: a tool's modules may define them after the shell is made
  const kinds = () => (typeof cfg.noticeKinds === 'function' ? cfg.noticeKinds() : cfg.noticeKinds) || {};
  function drawNotice() {
    const n = NOTICES[noticeKind] || kinds()[noticeKind];
    if (!n) return;
    nIcon.innerHTML = icon(n.icon);
    nText.textContent = n.text();
    nBtns.textContent = '';
    (typeof n.actions === 'function' ? n.actions() : n.actions).forEach(([name, run], i) => {
      const b = el('button', i === 0 ? 'btn primary' : 'btn soft', T(name));
      b.type = 'button';
      b.addEventListener('mousedown', (e) => e.preventDefault());
      // a run that answers true opened a menu from its button, and the row stays
      b.addEventListener('click', () => {
        sound();
        if (run && run(b) === true) return;
        hideNotice();
        if (!noticeKind) focusWork();
      });
      nBtns.appendChild(b);
    });
  }
  // the work is told when the row comes and goes, so it can move under it
  function showNotice(kind) {
    noticeKind = kind;
    drawNotice();
    notice.classList.add('on');
    if (cfg.noticeMoved) cfg.noticeMoved(true);
  }
  function hideNotice() {
    if (!noticeKind) return;
    noticeKind = '';
    notice.classList.remove('on');
    if (cfg.noticeMoved) cfg.noticeMoved(false);
  }
  /** A failed save of the tool's own (saving in place), said in the family's row */
  function saveFailed(kind) {
    failKind = kind || 'other';
    ctx.lastSave = null;
    showNotice('savefail');
  }
  /** A notice row of the tool's own, the same row as the family's and under it, in the order they're made.
   *  text: the Chinese words, or a function giving them; buttons: [[name, run, id], …], a button that only dismisses
   *  (run null) is the quiet one. Every button puts the row away. opts: { icon, id } (the id is the row's inside, the
   *  .notice-in). Returns { show(text?), hide(), on }. */
  const toolNotices = [];
  function makeNotice(text, buttons = [], { icon: ic = 'alert', id = '' } = {}) {
    const row = el('div', 'notice'),
      inner = el('div', 'notice-in'),
      i = el('span', 'notice-i'),
      t = el('p', 'notice-t'),
      bs = el('div', 'notice-b');
    inner.setAttribute('role', 'status');
    inner.append(i, t, bs);
    row.appendChild(inner);
    chrome.appendChild(row);
    if (id) inner.id = id;
    let shown = false,
      emptyT = 0;
    const n = {
      get on() {
        return shown;
      },
      draw() {
        if (!shown) return;
        i.innerHTML = icon(ic);
        t.textContent = T(typeof text === 'function' ? text() : text);
        bs.textContent = '';
        buttons.forEach(([name, run, bid, more]) => {
          const b = el('button', run ? 'btn primary' : 'btn soft', T(name));
          b.type = 'button';
          if (bid) b.id = bid;
          b.addEventListener('mousedown', (e) => e.preventDefault());
          // a button with { menu: true } opens a menu from itself, run(button), and the row stays (Text's Encoding)
          b.addEventListener('click', () => {
            sound();
            if (more && more.menu) {
              run(b);
              return;
            }
            n.hide();
            if (run) run();
          });
          bs.appendChild(b);
        });
      },
      show(now) {
        if (now !== undefined) text = now;
        shown = true;
        clearTimeout(emptyT);
        n.draw();
        row.classList.add('on');
      },
      // its buttons leave with it, once it has folded away: a row's buttons are only there while it shows
      hide() {
        if (!shown) return;
        shown = false;
        row.classList.remove('on');
        clearTimeout(emptyT);
        emptyT = setTimeout(() => {
          if (!shown) bs.textContent = '';
        }, 420);
      },
    };
    toolNotices.push(n);
    return n;
  }
  async function loadOutside() {
    const opened = await openPath(ctx.doc.path);
    if (opened) setState('已同步外部修改');
  }
  be.on('file-changed', (deleted) => {
    if (ctx.doc) showNotice(deleted ? 'deleted' : 'changed');
  });
  Object.assign(ctx, { kinds, drawNotice, showNotice, hideNotice, saveFailed, makeNotice, toolNotices });
  Object.defineProperties(ctx, {
    noticeKind: { get: () => noticeKind },
    failKind: {
      get: () => failKind,
      set: (v) => {
        failKind = v;
      },
    },
    kept: {
      get: () => kept,
      set: (v) => {
        kept = v;
      },
    },
  });
}
