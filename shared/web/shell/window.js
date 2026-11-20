// The window: the one top bar (the file's name and dot, Settings, Save As, the window's buttons), the stage the tool
// draws in, the empty view, the toast and the two menus' boxes. Made first: every other part hangs its things on these.
// cfg read here:
// - count: true — a quiet number before Settings that the tool fills with shell.setCount(text) (Text's words, Image's
//   n / m, Video's size); it goes with the rest of the top bar's own parts while the window is empty.
// - saveInPlace (state.js): Save beside the state, and Save As's tip says {Mod+Shift+S}.
// - focus(): give the work its focus back after the family's parts close (focusWork, Text's caret); settings.js keeps
//   the settings panel from taking it.
import { el, icon } from '../shared.js';
import { mountWindowControls } from '../window-frame.js';

/** Takes cfg from ctx. Gives every node it makes, headBtn (a top-bar button), focusWork and inPlace (cfg.saveInPlace). */
export function theWindow(ctx) {
  const { cfg } = ctx;
  // ---------- the window ----------
  const app = document.getElementById('app');
  const stage = el('div', 'stage');
  stage.id = 'stage';

  // the top: the same bar and the same buttons as every DotAny tool
  const chrome = el('div', 'chrome'),
    head = el('header', 'head'),
    where = el('div', 'where'),
    metaEl = el('div', 'meta');
  const dot = el('i', 'dot'),
    nameEl = el('span', 'name'),
    stateEl = el('span', 'state'),
    fileBtn = el('button', 'file');
  fileBtn.type = 'button';
  fileBtn.id = 'fileMenu';
  stateEl.setAttribute('aria-live', 'polite');
  fileBtn.append(nameEl);
  fileBtn.insertAdjacentHTML('beforeend', icon('chevron'));
  where.append(dot, fileBtn, stateEl);
  const inPlace = cfg.saveInPlace || null;
  // Save (保存) beside the state, for a tool that saves in place: it comes while there's something to save
  const saveBtn = inPlace ? el('button', 'save-now', '保存') : null;
  if (saveBtn) {
    saveBtn.type = 'button';
    saveBtn.id = 'saveNow';
    saveBtn.tabIndex = -1;
    where.append(saveBtn);
  }
  const headBtn = (id, n, label, tip) => {
    const b = el('button', 'tb');
    b.type = 'button';
    b.id = id;
    b.setAttribute('aria-label', label);
    b.dataset.tip = tip || label;
    b.innerHTML = icon(n) + `<span class="lab">${label}</span>`;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    return b;
  };
  const setBtn = headBtn('settings', 'settings', '设置'),
    saveAsBtn = headBtn('saveAs', 'saveAs', '另存为', inPlace ? '另存为 · {Mod+Shift+S}' : '另存为 · {Mod+S}');
  const focusWork = () => {
    if (cfg.focus) cfg.focus();
  };
  // a quiet number before Settings, for a tool that has one: the tool fills it with shell.setCount
  const countEl = cfg.count ? el('span', 'count dyn') : null;
  if (countEl) metaEl.append(countEl);
  metaEl.append(setBtn, saveAsBtn);
  head.append(where, metaEl);
  chrome.appendChild(head);
  mountWindowControls(head, metaEl, [where, stateEl, ...(countEl ? [countEl] : [])]);
  // the buttons' names fold into their icons when the top line hasn't room for them beside the file's name, in any language
  // or tool, and come back once it has: the room they need is measured whole, folded or not, so it never flickers
  let foldSeen = false;
  const fitNames = () => {
    const labs = [...metaEl.querySelectorAll('.lab')];
    // a name longer than the 8em the fold eases from (名前を付けて保存, Speichern unter) keeps its whole width: never cut
    labs.forEach((l) => l.style.setProperty('--lab-w', 'max(8em, ' + Math.ceil(l.scrollWidth) + 'px)'));
    const whole =
      metaEl.offsetWidth
      + labs.reduce((s, l) => s + l.scrollWidth - l.offsetWidth - (parseFloat(getComputedStyle(l).marginLeft) || 0), 0);
    const hs = getComputedStyle(head),
      room =
        head.clientWidth
        - parseFloat(hs.paddingLeft)
        - parseFloat(hs.paddingRight)
        - parseFloat(hs.columnGap || 0)
        - 160;
    if (!foldSeen) head.classList.add('lab-fold-now');
    head.classList.toggle('lab-fold', whole > room);
    if (!foldSeen) {
      foldSeen = true;
      requestAnimationFrame(() => requestAnimationFrame(() => head.classList.remove('lab-fold-now')));
    }
  };
  new ResizeObserver(fitNames).observe(head);
  new ResizeObserver(fitNames).observe(metaEl);

  const toast = el('div', 'toast'),
    emptyView = el('section', 'empty-view');
  toast.id = 'toast';
  app.append(stage, emptyView, toast, chrome);
  // the file menu, and after it the menu a tool opens from its own buttons
  const pop = el('div', 'pop'),
    menu = el('div', 'pop');
  menu.id = 'toolMenu';
  document.body.append(pop, menu);
  Object.assign(ctx, {
    app,
    stage,
    chrome,
    head,
    metaEl,
    dot,
    nameEl,
    stateEl,
    fileBtn,
    inPlace,
    saveBtn,
    headBtn,
    setBtn,
    saveAsBtn,
    focusWork,
    countEl,
    toast,
    emptyView,
    pop,
    menu,
  });
}
