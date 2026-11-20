// The file's state: what's open (ctx.doc, ctx.fileName), the little dot and the state beside it, and Save beside the
// state for a tool that saves in place.
// cfg read here:
// - dirty(): unsaved?; name: what the top bar says while nothing is open
// - saveInPlace: { save(), auto() } — the file is saved where it is: a Save button beside the state that the tool shows
//   with shell.saveReady(on), and shell.savedInPlace() when the tool saved it itself. save() answers true once it's on
//   disk; auto() is asked when the dot would turn dark and answers true if it saved by itself (auto-save). The rest of
//   it: Save {Mod+S} and Save As on {Mod+Shift+S} (opening.js), the menu (file-menu.js), the close prompt
//   (leaving.js).
import { reduce, EASE } from '../shared.js';
import { T } from '../i18n.js';
import { platform } from '../keys.js';
import { setEdited } from '../backend.js';

/** Takes from ctx: cfg and the window's nodes. Reads ctx.kept (notices.js). Gives dirty, setState, changed and the rest
 *  below, and ctx.doc, ctx.fileName and ctx.saving, which the other parts read and write. */
export function fileState(ctx) {
  const { cfg, stateEl, dot, saveAsBtn, inPlace, saveBtn, nameEl, focusWork } = ctx;
  // ---------- the file's state: the family's little dot ----------
  let doc = null,
    fileName = '';
  const dirty = () => !!doc && (ctx.kept || !!(cfg.dirty && cfg.dirty()));
  // orange the moment something changes, dark while it stays unsaved
  let nowTimer = 0,
    stateKey = '',
    stateArg = '',
    quietTimer = 0,
    stateAt = 0,
    saving = false;
  const renderState = () => {
    const text = stateKey === '已存为' ? T('已另存为 {name}', { name: stateArg }) : stateKey ? T(stateKey) : '';
    if (stateEl.textContent === text) return;
    stateEl.textContent = text;
    const now = performance.now();
    if (text && now - stateAt > 200 && !reduce)
      stateEl.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: EASE });
    stateAt = now;
  };
  function setState(k, arg = '') {
    stateKey = k;
    stateArg = arg;
    renderState();
    clearTimeout(quietTimer);
    stateEl.classList.remove('quiet');
    if (k && k !== '未保存' && k !== '正在读取…' && k !== '正在保存…')
      quietTimer = setTimeout(() => stateEl.classList.add('quiet'), 2600);
  }
  // the tool calls this after every change, and after undo and redo
  let edited = false;
  function changed() {
    const now = dirty();
    // the Mac also shows it in the red close button
    if (platform === 'mac' && now !== edited) {
      edited = now;
      setEdited(now);
    }
    // a tool that saves in place has its Save for that
    saveAsBtn.classList.toggle('hot', now && !inPlace);
    clearTimeout(nowTimer);
    if (saving) return;
    if (now) {
      dot.className = 'dot now';
      if (stateKey !== '未保存') setState('未保存');
      nowTimer = setTimeout(() => {
        if (inPlace && inPlace.auto && doc && dirty() && inPlace.auto()) return;
        dot.className = dirty() ? 'dot edited' : 'dot';
      }, 700);
    } else {
      dot.className = 'dot';
      if (stateKey === '未保存') setState('');
    }
  }
  /** The tool saved the file where it is (saving in place): what was kept is saved too */
  function savedInPlace() {
    ctx.kept = false;
  }
  function saveReady(on) {
    if (!saveBtn) return;
    saveBtn.classList.toggle('on', on);
    saveBtn.tabIndex = on ? 0 : -1;
  }
  if (saveBtn) {
    saveBtn.addEventListener('mousedown', (e) => e.preventDefault());
    saveBtn.addEventListener('click', () => {
      saveByHand();
      focusWork();
    });
  }
  // Save by hand: the tool's save, which knows a file with no name yet goes through Save As
  function saveByHand() {
    return doc ? Promise.resolve(inPlace.save()) : Promise.resolve(false);
  }
  function syncName() {
    nameEl.textContent = fileName || cfg.name;
  }
  Object.assign(ctx, { dirty, renderState, setState, changed, savedInPlace, saveReady, saveByHand, syncName });
  Object.defineProperties(ctx, {
    doc: {
      get: () => doc,
      set: (v) => {
        doc = v;
      },
    },
    fileName: {
      get: () => fileName,
      set: (v) => {
        fileName = v;
      },
    },
    saving: {
      get: () => saving,
      set: (v) => {
        saving = v;
      },
    },
  });
}
