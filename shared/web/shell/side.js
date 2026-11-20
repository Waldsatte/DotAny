// Side panels, opened from the tool's own top-bar buttons after the separator, and closing everything
// the family has open.
// cfg read here:
// - sideMorph: going from one side panel to another keeps the glass and morphs it: the height eases (`MOVE`, `--t-2`) and the new
//   contents fade in.
import { el, reduce, sound } from '../shared.js';
import { MOVE } from './morph.js';
import { later } from './later.js';

/** Takes from ctx: cfg, app, emptyView, metaEl, headBtn; calls closeSaveAs, closeSettings and closePop, and reads
 *  ctx.popFor. Gives headButton, sidePanel, closePanels and closeAll. */
export function sidePanels(ctx) {
  const { cfg, app, emptyView, metaEl, headBtn } = ctx;
  const { closeSaveAs, closeSettings, closePop } = later(ctx);
  // ---------- side panels ----------
  const panels = [];
  // the tool's own buttons come after a separator, before the window's, each one argued for
  let toolSep = null;
  function headButton(id, iconName, label, tip) {
    const b = headBtn(id, iconName, label, tip),
      winSep = metaEl.querySelector('.win-sep');
    if (!toolSep) {
      toolSep = el('i', 'sep');
      metaEl.insertBefore(toolSep, winSep);
    }
    metaEl.insertBefore(b, winSep);
    return b;
  }
  /** A side panel opened by a top-bar button after the separator. Returns the panel; the tool fills it. */
  function sidePanel(id, iconName, label) {
    const b = headButton(id + 'Btn', iconName, label);
    b.setAttribute('aria-pressed', 'false');
    const p = el('aside', 'side');
    p.id = id;
    app.insertBefore(p, emptyView);
    panels.push([p, b]);
    b.addEventListener('click', () => {
      sound();
      togglePanel(p, b);
    });
    return p;
  }
  function closePanels() {
    let was = false;
    for (const [p, b] of panels) {
      if (p.classList.contains('on')) was = true;
      p.classList.remove('on');
      b.setAttribute('aria-pressed', 'false');
    }
    return was;
  }
  function closeAll() {
    let was = closePanels();
    if (closeSaveAs(true)) was = true;
    if (closeSettings()) was = true;
    if (ctx.popFor) {
      closePop();
      was = true;
    }
    return was;
  }
  function togglePanel(p, b) {
    const on = !p.classList.contains('on'),
      was = on && cfg.sideMorph && !reduce ? panels.find(([q]) => q.classList.contains('on')) : null;
    if (was) {
      morphPanel(was[0], p, b);
      return;
    }
    closePanels();
    closeSaveAs(true);
    closeSettings();
    closePop();
    if (on) {
      p.classList.add('on');
      b.setAttribute('aria-pressed', 'true');
    }
  }
  // from one panel to the next the glass stays where it is: the old words go, the new ones fade in, the height eases (MOVE)
  function morphPanel(from, p, b) {
    const h = from.offsetHeight;
    [from, p].forEach((x) => {
      x.style.transition = 'none';
    });
    closePanels();
    closeSaveAs(true);
    closeSettings();
    closePop();
    p.classList.add('on');
    b.setAttribute('aria-pressed', 'true');
    // the tool fills the panel when it sees it open, so it's measured on the next frame, before anything is painted
    requestAnimationFrame(() => {
      void p.offsetWidth;
      [from, p].forEach((x) => {
        x.style.transition = '';
      });
      if (!p.classList.contains('on')) return;
      p.getAnimations().forEach((a) => a.cancel());
      const to = p.offsetHeight;
      [...p.children].forEach((c) => c.animate([{ opacity: 0 }, { opacity: 1 }], MOVE));
      if (to === h) return;
      p.style.overflow = 'hidden';
      const a = p.animate([{ height: h + 'px' }, { height: to + 'px' }], MOVE);
      a.onfinish = a.oncancel = () => {
        p.style.overflow = '';
      };
    });
  }
  Object.assign(ctx, { headButton, sidePanel, closePanels, closeAll });
}
