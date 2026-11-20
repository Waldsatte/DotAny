// Leaving with unsaved work: the close prompt ("Save changes to “…”?"), asked before closing, opening another file or
// making a new one; and its Save with UTF-8 form.
// cfg read here:
// - discard(): forget the unsaved changes (Don't Save)
// - beforeGuard(): the tool's last word before unsaved work is asked about (Image commits text being typed, ends a
//   crop).
// - saveInPlace: the close prompt's Save saves in place (a file with no name yet goes through Save As).
// shell.askEncoding(titleParts, retry): the close prompt asks to Save with UTF-8 a file whose encoding can't hold
// what's in it.
import * as be from '../backend.js';
import { el, sound } from '../shared.js';
import { T } from '../i18n.js';
import { platform, used } from '../keys.js';

/** Takes from ctx: cfg, dirty, dot, inPlace, saveAsBtn, setState, focusWork, openSaveAs; reads ctx.fileName and ctx.doc
 *  and writes ctx.kept. Gives ask, guard and askEncoding. */
export function leaving(ctx) {
  const { cfg, dirty, dot, inPlace, saveAsBtn, setState, focusWork, openSaveAs } = ctx;
  // ---------- leaving with unsaved work ----------
  const ask = el('div', 'ask-wrap');
  ask.setAttribute('role', 'alertdialog');
  ask.setAttribute('aria-modal', 'true');
  const askBox = el('div', 'ask'),
    askTitle = el('p', 'ask-title dyn'),
    askRow = el('div', 'ask-row');
  const aDrop = el('button', 'ask-drop', '不保存'),
    aCancel = el('button', 'btn soft', '取消'),
    aSave = el('button', 'btn primary', '保存');
  const aUtf8 = el('button', 'btn primary', '转为 UTF-8 保存');
  [aDrop, aCancel, aSave, aUtf8].forEach((b) => {
    b.type = 'button';
  });
  aDrop.id = 'askDiscard';
  aCancel.id = 'askCancel';
  aSave.id = 'askSave';
  aUtf8.id = 'askUtf8';
  const askGap = el('i', 'ask-gap');
  askRow.append(aDrop, askGap, aCancel, aSave);
  askBox.append(askTitle, askRow);
  ask.appendChild(askBox);
  document.body.appendChild(ask);
  let askNext = null,
    askRetry = null;
  function guard(next) {
    if (cfg.beforeGuard) cfg.beforeGuard();
    if (!dirty()) {
      next();
      return;
    }
    askNext = next;
    // the file's name sits inside the sentence, where the language puts it
    const [before, after] = T('是否保存对「{name}」的更改？').split('{name}');
    askTitle.replaceChildren(el('span', '', before), el('span', 'ask-name', ctx.fileName), el('span', '', after));
    ask.classList.remove('tabbing');
    ask.classList.add('on');
    aSave.focus();
  }
  /** What can't be written in the file's encoding: the prompt asks to Save with UTF-8.
   *  title: the words, as nodes; retry(): save again in UTF-8, true once it's on disk. What the prompt was asked for
   *  (closing, opening another file) goes on after that. */
  function askEncoding(title, retry) {
    askRetry = retry;
    askTitle.replaceChildren(...title);
    ask.classList.remove('tabbing');
    aUtf8.textContent = T('转为 UTF-8 保存');
    askRow.replaceChildren(askGap, aCancel, aUtf8);
    ask.classList.add('on');
    aUtf8.focus();
  }
  const hideAsk = () => {
    ask.classList.remove('on');
    askRow.replaceChildren(aDrop, askGap, aCancel, aSave);
    askRetry = null;
  };
  // the question was asked because the window was closing (its red button, ⌘W, Quit, a logout): Cancel keeps the
  // window, and on the Mac a quit under way stops at once, a logout waiting for the program is called off
  // (be.windowControl 'stay'). Windows ends a program with its window and has nothing to tell
  let closing = false;
  const stays = () => {
    if (closing && platform === 'mac') be.windowControl('stay');
    closing = false;
  };
  ctx.closeGivenUp = stays;
  Object.defineProperty(ctx, 'closing', { get: () => closing });
  aCancel.addEventListener('click', () => {
    sound();
    hideAsk();
    askNext = null;
    stays();
    focusWork();
  });
  aDrop.addEventListener('click', () => {
    sound();
    hideAsk();
    if (cfg.discard) cfg.discard();
    ctx.kept = false;
    dot.className = 'dot';
    if (!inPlace) setState('');
    const n = askNext;
    askNext = null;
    if (n) n();
  });
  aSave.addEventListener('click', async () => {
    if (inPlace) sound();
    hideAsk();
    // in place, when the file has a name: if the save can't finish, nothing goes on and the reason stays on screen
    if (inPlace && ctx.doc && ctx.doc.path) {
      const n = askNext;
      if (await inPlace.save()) {
        askNext = null;
        if (n) n();
      }
      return;
    }
    const n = askNext;
    askNext = null;
    await openSaveAs(saveAsBtn, n);
  });
  aUtf8.addEventListener('click', async () => {
    sound();
    const retry = askRetry;
    hideAsk();
    const n = askNext;
    if (retry && (await retry())) {
      askNext = null;
      if (n) n();
    }
  });
  ask.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Escape') {
      used(e);
      sound();
      hideAsk();
      askNext = null;
      stays();
      focusWork();
      return;
    }
    if (e.key !== 'Tab') return;
    ask.classList.add('tabbing');
    // the keyboard stays among the answers shown
    const list = [...askRow.querySelectorAll('button')],
      at = list.indexOf(document.activeElement);
    e.preventDefault();
    list[(at + (e.shiftKey ? list.length - 1 : 1)) % list.length].focus();
  });
  be.on('close-requested', () => {
    closing = true;
    guard(() => be.closeWindow());
  });
  Object.assign(ctx, { ask, guard, askEncoding });
}
