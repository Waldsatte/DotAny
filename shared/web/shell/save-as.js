// Save as: the card under Save As, with the system's save dialog asked once the card is answered, and the toast
// (say).
// cfg read here:
// - formats (FORMAT, given by createShell): { key: { ext: '.mp3', label: 'MP3' } }; formatOf(doc): the format a file
//   opens in. Formats with two-part extensions (.tar.gz) are known as such by the card's name
// - save(path, fmt): write it, answering { status: 'saved', path, name } or { status: 'failed', kind }; saved(r, fmt):
//   after a save
// - estimate(fmt): the new file's size in bytes, or { size, note } to say a little more after it (", 12 pictures"; in
//   Slides); saveRows(box, fmt): the card's options; saveNote(fmt): its quiet line; money(bytes): the tool's way of
//   writing a size (Image: 12 MB from 10 MB).
// - a format with export: true, or a save answering exported: true, is a copy: the window stays on the open file, the
//   line says Saved as and the copy's name, and the tool's Rust leaves it off the recent list (it doesn't call
//   dotany_base::saved).
// - oneFormat: true or (doc) => true — the card keeps formatOf(doc) and doesn't show the format row.
// - formatsFor(doc): the format keys this file can be saved as, for a card whose formats depend on the file.
// - liveNote: the card's quiet line comes from the tool, not from saveNote. saveLine(fmt) gives it at once (a promise
//   of the line already in the language shown, or undefined: the tool knows it without encoding anything), and the
//   estimate keeps it current as the options change, answering { size, line }. A format switch is one motion: the card
//   waits for saveLine, then the line and the rows change together and the size follows alone; the card opens with the
//   line already in place. saveLine(fmt) and estimate(fmt) describe fmt from the tool's state as it is, before
//   saveRows has drawn fmt.
// - cardScrolls: a card taller than the window scrolls. dimWhileSaving: the card stays while writing, Save dimmed.
// - beforeSaveAs(): the tool's last word before the card opens (Image commits text being typed, ends a crop).
// - noteAtFoot: a tool with no estimate says the format's line (saveNote) at the card's foot, where About {size} would
//   be.
// - askFirst: the card stays while the file is written, and save(path, fmt) may answer { status: 'ask' }: the tool asks
//   something first (Text: Save with UTF-8) and the card stays; shell.retrySaveAs() writes again. A save answering
//   status 'copied' is a copy: Saved as, and the window stays.
import * as be from '../backend.js';
import { el, sound, reduce, restingRect } from '../shared.js';
import { T, size as sizeText } from '../i18n.js';
import { inkfall } from '../inkfall-input.js';
import { setWindowTitle } from '../window-frame.js';
import { os, platform, used } from '../keys.js';
import { morph, MOVE } from './morph.js';
import { later } from './later.js';

/** Takes from ctx: cfg, FORMAT, the window's nodes, the file's state, closePanels; calls the notices' and the menus'
 *  functions; reads and writes ctx.doc, ctx.fileName, ctx.saving, ctx.kept, ctx.failKind, ctx.noticeKind. Gives the
 *  card's functions below, say, and ctx.dlgOpen, ctx.fmtBtns, ctx.lastSave and ctx.testSave. */
export function saveAs(ctx) {
  const { cfg, FORMAT, saveAsBtn, fileBtn, toast, focusWork, setState, changed, syncName, closePanels } = ctx;
  const { closePop, closeSettings, showNotice, hideNotice, kinds } = later(ctx);
  // ---------- save as ----------
  const dlg = el('div', 'dlg');
  dlg.setAttribute('role', 'dialog');
  dlg.setAttribute('aria-label', '另存为');
  const dlgInput = document.createElement('input');
  dlgInput.type = 'text';
  dlgInput.id = 'saveAsName';
  dlgInput.spellcheck = false;
  dlgInput.autocomplete = 'off';
  dlgInput.setAttribute('aria-label', '文件名');
  const fieldIn = el('span', 'field-in');
  fieldIn.appendChild(dlgInput);
  const extEl = el('span', 'ext'),
    field = el('label', 'field');
  field.append(fieldIn, extEl);
  const fmtSeg = el('div', 'seg'),
    fmtPill = el('i', 'pl');
  fmtSeg.appendChild(fmtPill);
  const noteEl = el('p', 'sv-note dyn none');
  noteEl.id = 'saveNote';
  const rows = el('div', 'sv-rows');
  const estEl = el('p', 'warn-t'),
    warn = el('div', 'warn');
  warn.appendChild(estEl);
  estEl.id = 'estimate';
  const cancelBtn = el('button', 'btn', '取消'),
    confirmBtn = el('button', 'btn primary', '保存');
  cancelBtn.type = 'button';
  confirmBtn.type = 'button';
  confirmBtn.id = 'saveAsConfirm';
  cancelBtn.id = 'saveAsCancel';
  const foot = el('div', 'dlg-foot');
  foot.append(warn, cancelBtn, confirmBtn);
  dlg.append(field, fmtSeg, noteEl, rows, foot);
  document.body.appendChild(dlg);
  inkfall(dlgInput);
  let dlgOpen = false,
    fmtKey = Object.keys(FORMAT)[0],
    whenSaved = null,
    picking = false,
    folder = '',
    lastSave = null;
  let fmtBtns = [];
  // the format buttons: every format, or (formatsFor) only those this file can become
  function drawFormats(keys) {
    fmtBtns.forEach((b) => b.remove());
    fmtBtns = keys.map((k) => {
      const F = FORMAT[k],
        b = el('button', 'sg', T(F.label));
      b.type = 'button';
      b.dataset.k = k;
      b.id = 'fmt-' + k;
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', () => {
        sound();
        fmtKey = k;
        syncDlg(true);
      });
      fmtSeg.appendChild(b);
      return b;
    });
  }
  drawFormats(Object.keys(FORMAT));
  // one format needs no choosing
  const oneHidden = () => fmtBtns.length < 2;
  if (oneHidden()) fmtSeg.hidden = true;
  const measureText = document.createElement('canvas').getContext('2d');
  function fitName() {
    const cs = getComputedStyle(dlgInput);
    measureText.font = cs.fontWeight + ' ' + cs.fontSize + ' ' + cs.fontFamily;
    fieldIn.style.width = Math.ceil(measureText.measureText(dlgInput.value).width) + 3 + 'px';
  }
  function nameProblem() {
    const v = dlgInput.value.trim();
    if (!v) return '文件名不能为空。';
    // the Mac refuses only / and : in a name (Finder shows a : as /)
    if ((platform === 'mac' ? /[/:]/ : /[\\/:*?"<>|]/).test(v))
      return os({ windows: '文件名不能包含以下字符：\\ / : * ? " < > |', mac: '文件名不能包含「/」或「:」。' });
    return '';
  }
  const money = cfg.money || ((b) => sizeText(b));
  let estSeq = 0,
    estAt = 0,
    askedAt = 0,
    estWas = null;
  function estimate() {
    const problem = dlgOpen ? nameProblem() : '';
    warn.classList.toggle('bad', !!problem);
    field.classList.toggle('bad', !!problem);
    if (problem) {
      setEst(T(problem));
      return;
    }
    // no estimate: the format's line is said here
    if (cfg.noteAtFoot && ctx.doc && dlgOpen) {
      setEst(T((cfg.saveNote && cfg.saveNote(fmtKey)) || ''));
      return;
    }
    if (!ctx.doc || !dlgOpen || !cfg.estimate) return;
    const seq = ++estSeq;
    // asked again within 200 ms of the last asking: a figure being dragged, which changes where it stands
    const now = performance.now(),
      dragged = now - askedAt < 200;
    askedAt = now;
    Promise.resolve(cfg.estimate(fmtKey)).then((size) => {
      let note = '',
        line;
      if (size && typeof size === 'object') {
        note = size.note || '';
        line = size.line;
        size = size.size;
      }
      if (size == null || seq !== estSeq || !dlgOpen || nameProblem()) return;
      setEst(T('预计 {size}', { size: money(size) }) + note, dragged);
      // the quiet line under the formats, when what it says comes with the estimate. While a
      // format switch waits for its line, that switch says it, with the rows
      if (line !== undefined && !lineAsked) setNote(line);
    });
  }
  function putNote(text) {
    noteEl.textContent = text;
    noteEl.classList.toggle('none', !text);
  }
  function setNote(text) {
    if (noteEl.textContent !== text)
      morph(
        noteEl,
        () => putNote(text),
        () => [noteEl]
      );
  }
  // a row is the same row when it has the same name and says the same: it stays still while the others come and go
  const rowKey = (r) => (r.id || (r.querySelector('[id]') || {}).id || '') + '|' + r.className + '|' + r.textContent;
  // another format: the quiet line and the rows change in one motion, and only what is new fades in (line undefined:
  // the line stays as it is)
  function switchRows(line) {
    // where each row sits among the rows, before and after
    const at = (r) => r.getBoundingClientRect().top - rows.getBoundingClientRect().top;
    const had = new Map([...rows.children].map((r) => [rowKey(r), at(r)])),
      said = noteEl.textContent;
    morph(
      [noteEl, rows],
      () => {
        if (line !== undefined) putNote(line);
        drawRows();
      },
      () => [
        ...[...rows.children].filter((r) => !had.has(rowKey(r))),
        noteEl.textContent && noteEl.textContent !== said ? noteEl : null,
      ]
    );
    // a row that stays moves to its new place with the rest (the same MOVE), it doesn't land there at once
    if (!reduce)
      for (const r of rows.children) {
        const from = had.get(rowKey(r)),
          by = from == null ? 0 : from - at(r);
        if (by) r.animate([{ transform: `translateY(${by}px)` }, { transform: 'none' }], MOVE);
      }
  }
  // the quiet line for a liveNote tool, at once; undefined when the tool can't say it
  const lineFor = (fmt) =>
    cfg.saveLine ? Promise.resolve(cfg.saveLine(fmt)).catch(() => undefined) : Promise.resolve(undefined);
  let lineSeq = 0,
    lineAsked = false;
  // a new figure fades in while the one before fades out where it stood, so the line is never empty; except while it's
  // changing faster than the eye can follow, or being dragged (still)
  function setEst(text, still) {
    if (estEl.textContent === text) return;
    const said = estEl.textContent;
    estEl.textContent = text;
    const now = performance.now();
    if (!still && now - estAt > 200) {
      if (estWas) estWas.remove();
      estWas = null;
      if (said && !reduce) {
        const was = (estWas = el('p', 'warn-t was', said));
        was.setAttribute('aria-hidden', 'true');
        warn.appendChild(was);
        was.animate([{ opacity: 1 }, { opacity: 0 }], MOVE).onfinish = () => {
          was.remove();
          if (estWas === was) estWas = null;
        };
      }
      estEl.classList.remove('swap');
      void estEl.offsetWidth;
      estEl.classList.add('swap');
    }
    estAt = now;
  }
  function drawRows() {
    rows.textContent = '';
    // which format the rows are drawn for (a liveNote tool's draw a moment after the click)
    rows.dataset.fmt = fmtKey;
    if (cfg.saveRows) cfg.saveRows(rows, fmtKey, estimate);
  }
  function placeFmtPill() {
    const on = fmtBtns.find((b) => b.dataset.k === fmtKey);
    if (!on) return;
    fmtPill.style.transform = `translateX(${on.offsetLeft}px)`;
    fmtPill.style.width = on.offsetWidth + 'px';
  }
  function syncDlg(changedFmt) {
    fitName();
    fmtBtns.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.k === fmtKey)));
    const F = FORMAT[fmtKey];
    if (extEl.textContent !== F.ext) {
      extEl.textContent = F.ext;
      if (changedFmt) {
        extEl.classList.remove('swap');
        void extEl.offsetWidth;
        extEl.classList.add('swap');
      }
    }
    placeFmtPill();
    const line = cfg.liveNote || cfg.noteAtFoot ? undefined : cfg.saveNote ? T(cfg.saveNote(fmtKey) || '') : '';
    if (changedFmt && cfg.liveNote) {
      // the line is asked for first (no encoding, a moment), so it and the rows move once; the size comes later alone
      const asked = ++lineSeq,
        fmt = fmtKey;
      lineAsked = true;
      lineFor(fmt).then((said) => {
        if (asked !== lineSeq) return;
        lineAsked = false;
        if (!dlgOpen || fmt !== fmtKey) return;
        switchRows(said);
        // no line: the estimate says it
        if (said === undefined) estimate();
      });
    } else if (changedFmt) switchRows(line);
    else if (line !== undefined) setNote(line);
    estimate();
  }
  async function openSaveAs(anchor = saveAsBtn, done = null) {
    if (!ctx.doc) return;
    if (cfg.beforeSaveAs) cfg.beforeSaveAs();
    closePanels();
    closePop();
    closeSettings();
    whenSaved = done;
    if (cfg.formatsFor) drawFormats(cfg.formatsFor(ctx.doc));
    // a two-part extension (.tar.gz, and any compressed .tar.xz the tool opens) goes as a whole, otherwise what follows the last dot
    const two =
      Object.values(FORMAT)
        .map((F) => F.ext)
        .find(
          (x) =>
            x.split('.').length > 2
            && ctx.fileName.length > x.length
            && ctx.fileName.toLowerCase().endsWith(x.toLowerCase())
        ) || (ctx.fileName.match(/.(\.tar\.(gz|bz2|xz|zst))$/i) || [])[1];
    const at = two ? ctx.fileName.length - two.length : ctx.fileName.lastIndexOf('.');
    dlgInput.value = at > 0 ? ctx.fileName.slice(0, at) : ctx.fileName;
    const offered = fmtBtns.map((b) => b.dataset.k);
    fmtKey = (cfg.formatOf && cfg.formatOf(ctx.doc)) || offered[0];
    if (!offered.includes(fmtKey)) fmtKey = offered[0];
    fmtSeg.hidden = oneHidden() || !!(typeof cfg.oneFormat === 'function' ? cfg.oneFormat(ctx.doc) : cfg.oneFormat);
    // a liveNote tool's line is there as the card opens, so nothing moves under it once it's shown
    lineSeq++;
    lineAsked = false;
    if (cfg.liveNote) {
      const [where, line] = await Promise.all([ctx.doc.dir || be.defaultFolder(), lineFor(fmtKey)]);
      folder = where;
      putNote(line || '');
    } else folder = ctx.doc.dir || (await be.defaultFolder());
    dlgOpen = true;
    sound();
    const r = restingRect(anchor);
    drawRows();
    dlg.style.top = Math.round(r.bottom + 8) + 'px';
    const fromRight = anchor !== fileBtn,
      want = fromRight ? r.right - dlg.offsetWidth : r.left;
    dlg.style.transformOrigin = fromRight ? '100% 0' : '0 0';
    dlg.style.left = Math.round(Math.max(8, Math.min(innerWidth - dlg.offsetWidth - 8, want))) + 'px';
    // a card taller than the window scrolls inside it
    if (cfg.cardScrolls) {
      dlg.style.maxHeight = `calc(100% - ${r.bottom + 16}px)`;
      dlg.style.overflow = 'auto';
    }
    // Save As stays pressed while its card is open, as Settings does
    saveAsBtn.setAttribute('aria-pressed', String(fromRight));
    extEl.textContent = '';
    syncDlg(false);
    dlg.classList.add('on');
    setTimeout(() => {
      placeFmtPill();
      dlgInput.focus();
      dlgInput.select();
    }, 30);
  }
  function closeSaveAs(quietly) {
    if (!dlgOpen) return false;
    if (quietly !== true) sound();
    // put away unsaved, while the window was waiting on it to close: the close was given up (leaving.js)
    if (quietly !== true && whenSaved && ctx.closing && ctx.closeGivenUp) ctx.closeGivenUp();
    dlgOpen = false;
    whenSaved = null;
    saveAsBtn.removeAttribute('aria-pressed');
    dlg.classList.remove('on');
    dlgInput.blur();
    focusWork();
    return true;
  }
  const cardOpen = () => dlgOpen;
  function say(text, ms = 4200) {
    toast.textContent = text;
    toast.classList.add('on');
    clearTimeout(say.t);
    say.t = setTimeout(() => toast.classList.remove('on'), ms);
  }
  async function writeFile(path, fmt, after, written) {
    ctx.saving = true;
    setState('正在保存…');
    const r = await cfg.save(path, fmt);
    ctx.saving = false;
    lastSave = { path, fmt, after };
    // the tool asks something first: the card stays, and shell.retrySaveAs() writes again
    if (r.status === 'ask') {
      setState('');
      changed();
      if (written) written(false);
      return false;
    }
    // a copy, as the tool says it (Text's long files): Saved as, the window stays
    if (r.status === 'copied') {
      r.status = 'saved';
      r.exported = true;
    }
    if (written) written(r.status === 'saved');
    if (r.status !== 'saved') {
      setState('');
      changed();
      ctx.failKind = r.status === 'failed' ? r.kind : r.status;
      lastSave = { path, fmt, after };
      showNotice('savefail');
      return false;
    }
    // what was said about the old file goes; what the tool says about this one (Text's encoding) stays
    if (!kinds()[ctx.noticeKind]) hideNotice();
    // a copy (a picture, a PDF, a filtered table) isn't the file: the window stays where it is
    if (FORMAT[fmt].export) r.exported = true;
    if (!r.exported) {
      ctx.fileName = r.name;
      syncName();
      setWindowTitle(r.name, r.path);
      ctx.doc.path = r.path;
      ctx.doc.dir = r.path.replace(/[\\/][^\\/]*$/, '');
    }
    ctx.kept = false;
    // the tool may say it itself (Text: Saved, when the file was saved as itself)
    const said = cfg.saved ? cfg.saved(r, fmt) : null;
    changed();
    if (said !== 'said') setState('已存为', r.name);
    sound();
    if (after) after();
    return true;
  }
  // where it goes is the system's save dialog's question, asked once the panel is answered
  async function confirmSaveAs() {
    if (picking || ctx.saving) return;
    if (nameProblem()) {
      sound();
      estimate();
      dlgInput.focus();
      return;
    }
    sound();
    const ext = FORMAT[fmtKey].ext,
      name = dlgInput.value.trim() + ext,
      label = FORMAT[fmtKey].label,
      fmt = fmtKey,
      after = whenSaved;
    picking = true;
    const chosen = testSave ? testSave(name) : await be.pickSave(folder, name, ext, T(label));
    picking = false;
    if (!chosen) {
      dlgInput.focus();
      return;
    }
    // a film keeps its own extension as it's written (.MP4): the dialog's answer is matched without regard to case
    const target = chosen.toLowerCase().endsWith(ext.toLowerCase()) ? chosen : chosen + ext;
    // a large file takes a while: the card stays, its Save dimmed and still, until it's written
    if (cfg.dimWhileSaving) {
      confirmBtn.disabled = true;
      confirmBtn.style.opacity = 0.5;
      await writeFile(target, fmt, after, () => {
        confirmBtn.disabled = false;
        confirmBtn.style.opacity = '';
        closeSaveAs(true);
      });
      return;
    }
    // a tool that may ask first keeps the card until the file is written
    if (cfg.askFirst) {
      await writeFile(target, fmt, after, (ok) => {
        if (ok) closeSaveAs(true);
      });
      return;
    }
    closeSaveAs(true);
    writeFile(target, fmt, after);
  }
  /** Writes again where the card last wrote, once the tool's question is answered. True once on disk. */
  function retrySaveAs() {
    if (!lastSave) return Promise.resolve(false);
    const { path, fmt, after } = lastSave;
    return writeFile(path, fmt, after, (ok) => {
      if (ok) closeSaveAs(true);
    });
  }
  let testSave = null;
  dlgInput.addEventListener('input', () => {
    fitName();
    estimate();
  });
  dlgInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      confirmSaveAs();
    }
  });
  dlg.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Escape') {
      used(e);
      closeSaveAs();
    }
  });
  // the card's buttons never take the focus: a focused button in a closed card would keep every
  // key from the window
  [cancelBtn, confirmBtn].forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));
  cancelBtn.addEventListener('click', () => closeSaveAs());
  confirmBtn.addEventListener('click', confirmSaveAs);
  saveAsBtn.addEventListener('click', () => {
    if (cardOpen()) closeSaveAs();
    else openSaveAs(saveAsBtn);
  });
  document.addEventListener('pointerdown', (e) => {
    if (
      dlgOpen
      && !picking
      && !ctx.saving
      && !dlg.contains(e.target)
      && !fileBtn.contains(e.target)
      && !saveAsBtn.contains(e.target)
    )
      closeSaveAs();
  });
  Object.assign(ctx, { dlg, drawFormats, drawRows, syncDlg, openSaveAs, closeSaveAs, cardOpen, say, retrySaveAs });
  Object.defineProperties(ctx, {
    dlgOpen: { get: () => dlgOpen },
    fmtBtns: { get: () => fmtBtns },
    lastSave: {
      get: () => lastSave,
      set: (v) => {
        lastSave = v;
      },
    },
    testSave: {
      get: () => testSave,
      set: (v) => {
        testSave = v;
      },
    },
  });
}
