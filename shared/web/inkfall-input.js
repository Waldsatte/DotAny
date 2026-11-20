// The typing feel for native <input> fields (a drawn caret that glides, new letters that fade in) — framework-free.
// While a field has focus, its words are drawn by a layer laid exactly over it (so letters can settle in) with a caret
// that glides; the native field keeps the value, the selection and all keyboard behaviour. On blur it shows its own text
// again, so a value changed from code while unfocused is never shown stale.
//
// Usage:   import { inkfallAuto } from './inkfall-input.js'; inkfallAuto('input.field');   // every matching field
//    or:   inkfall(inputElement);                                                       // one field
// Requirements: the input sits in a wrapper with `position: relative` (the label/field container). Single-line inputs;
// for <textarea>, render the layer with `white-space: pre-wrap`, the same width, and measure with a Range the same way.
import { REST, FRESH, glideStep, farJump, changeOf, mapRange } from './core.js';

const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
// the theme can change while a field is in use (from settings, or from the system): its ink follows. Only a real change
// counts: reading the ink again touches the field's classes, and a page that answers class changes by setting its own on the
// root (Books' top bar) would otherwise go round forever
const recolors = new Set();
let shade = '';
const recolorAll = () => {
  const r = document.documentElement,
    s = [r.dataset.theme, r.className, matchMedia('(prefers-color-scheme: dark)').matches].join(' ');
  if (s === shade) return;
  shade = s;
  recolors.forEach((f) => f());
};
new MutationObserver(recolorAll).observe(document.documentElement, {
  attributes: true,
  attributeFilter: ['data-theme', 'class'],
});
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', recolorAll);
const COPY = [
  'fontFamily',
  'fontSize',
  'fontWeight',
  'fontStyle',
  'letterSpacing',
  'wordSpacing',
  'textTransform',
  'lineHeight',
  'fontVariantNumeric',
  'fontFeatureSettings',
];

export function inkfall(input) {
  if (input._inkfall) return input._inkfall;
  const wrap = input.parentElement;
  const ink = document.createElement('span'),
    tx = document.createElement('span'),
    caret = document.createElement('i');
  ink.className = 'inkfall-layer';
  caret.className = 'inkfall-caret';
  ink.setAttribute('aria-hidden', 'true');
  caret.setAttribute('aria-hidden', 'true');
  ink.appendChild(tx);
  wrap.append(ink, caret);

  let value = input.value,
    fresh = [],
    compFrom = -1,
    cur = null,
    raf = 0,
    last = 0,
    movedAt = 0,
    sig = '',
    box = null,
    settle = 0;

  function place() {
    const cs = getComputedStyle(input);
    for (const k of COPY) ink.style[k] = cs[k];
    const pl = parseFloat(cs.paddingLeft) + parseFloat(cs.borderLeftWidth),
      pt = parseFloat(cs.paddingTop) + parseFloat(cs.borderTopWidth);
    const w = input.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const h = input.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    box = { left: input.offsetLeft + pl, top: input.offsetTop + pt, w, h, fs: parseFloat(cs.fontSize) };
    Object.assign(ink.style, {
      left: box.left + 'px',
      top: box.top + 'px',
      width: w + 'px',
      height: h + 'px',
      lineHeight: h + 'px',
    });
  }
  // the words; fresh ones wrapped, their age as a negative delay so a redraw continues instead of restarting
  function draw() {
    const now = performance.now();
    fresh = fresh.filter((f) => now - f.t < FRESH);
    tx.textContent = '';
    let at = 0;
    for (const f of [...fresh].sort((a, b) => a.from - b.from)) {
      if (f.from < at) continue;
      if (f.from > at) tx.append(value.slice(at, f.from));
      const s = document.createElement('span');
      s.className = 'inkfall-fresh';
      s.style.animationDelay = `-${Math.round(now - f.t)}ms`;
      s.textContent = value.slice(f.from, f.to);
      tx.appendChild(s);
      at = f.to;
    }
    if (at < value.length) tx.append(value.slice(at));
    clearTimeout(settle);
    if (fresh.length)
      settle = setTimeout(() => {
        if (wrap.classList.contains('is-inkfall')) draw();
      }, FRESH + 20);
  }
  function target() {
    if (input.selectionStart !== input.selectionEnd) return null; // a selection: no caret, like Word
    let off = input.selectionStart ?? value.length,
      x = 0,
      node;
    const walker = document.createTreeWalker(tx, NodeFilter.SHOW_TEXT);
    while ((node = walker.nextNode())) {
      if (off <= node.length) {
        const r = document.createRange();
        r.setStart(node, off);
        r.setEnd(node, off);
        x = r.getBoundingClientRect().left - tx.getBoundingClientRect().left;
        break;
      }
      off -= node.length;
    }
    if (!node && tx.lastChild) x = tx.getBoundingClientRect().width;
    const h = Math.min(box.h, box.fs * 1.25);
    return { x: box.left + x - input.scrollLeft, y: box.top + (box.h - h) / 2, h };
  }
  function frame(now) {
    raf = 0;
    if (!wrap.classList.contains('is-inkfall')) return;
    const dt = Math.min(64, now - last);
    last = now;
    if (input.value !== value) {
      const c = changeOf(value, input.value);
      fresh = fresh.map((f) => mapRange(f, c)).filter(Boolean);
      value = input.value;
      draw();
    }
    const s = `${value.length}|${input.selectionStart}|${input.selectionEnd}|${input.scrollLeft}|${input.clientWidth}`;
    if (s !== sig) {
      sig = s;
      place();
      tx.style.transform = `translateX(${-input.scrollLeft}px)`;
    }
    const t = target();
    caret.classList.toggle('on', !!t);
    if (t) {
      const was = cur;
      let moving = false;
      if (reduce || farJump(cur, t, t.h)) cur = { ...t };
      else ({ pos: cur, moving } = glideStep(cur, t, dt));
      if (moving || !was || was.x !== cur.x || was.y !== cur.y) movedAt = now;
      caret.style.transform = `translate(${cur.x}px, ${cur.y}px)`;
      caret.style.height = cur.h + 'px';
      caret.classList.toggle('is-blink', now - movedAt > REST);
    } else cur = null;
    raf = requestAnimationFrame(frame); // a field is small: watch it while focused (values set from code, scroll, selection)
  }
  function kick() {
    movedAt = performance.now();
    caret.classList.remove('is-blink');
    if (!raf) {
      last = performance.now();
      raf = requestAnimationFrame(frame);
    }
  }

  input.addEventListener('input', (e) => {
    const c = changeOf(value, input.value);
    fresh = fresh.map((f) => mapRange(f, c)).filter(Boolean);
    // typed letters settle in; pastes, deletions and text still being composed do not
    if (
      !reduce
      && !e.isComposing
      && /^insert(Text|ReplacementText)$/.test(e.inputType || 'insertText')
      && c.newTo > c.from
    ) {
      fresh.push({ from: c.from, to: c.newTo, t: performance.now() });
    }
    value = input.value;
    draw();
    kick();
  });
  input.addEventListener('compositionstart', () => {
    compFrom = input.selectionStart;
  });
  input.addEventListener('compositionend', () => {
    const from = compFrom;
    compFrom = -1;
    setTimeout(() => {
      value = input.value;
      const to = input.selectionStart;
      if (!reduce && from >= 0 && to > from) fresh.push({ from, to, t: performance.now() });
      draw();
      kick();
    }, 0);
  });
  input.addEventListener('keydown', kick);
  input.addEventListener('pointerdown', kick);
  input.addEventListener('focus', on);
  input.addEventListener('blur', () => {
    wrap.classList.remove('is-inkfall');
    cancelAnimationFrame(raf);
    raf = 0;
    cur = null;
    fresh = [];
  });
  function on() {
    if (input.readOnly || input.disabled) return;
    value = input.value;
    fresh = [];
    sig = '';
    cur = null;
    color();
    place();
    draw();
    wrap.classList.add('is-inkfall');
    kick();
  }
  // the field's own color, read while it is still showing it, where any transition will land
  function color() {
    const was = wrap.classList.contains('is-inkfall'),
      tr = input.style.transition;
    wrap.classList.remove('is-inkfall');
    input.style.transition = 'none';
    ink.style.color = getComputedStyle(input).color;
    input.style.transition = tr;
    if (was) wrap.classList.add('is-inkfall');
  }
  recolors.add(() => {
    if (wrap.classList.contains('is-inkfall')) color();
  });
  input._inkfall = on;
  if (document.activeElement === input) on();
  return on;
}

// attach lazily to every matching text field the first time it is focused, including ones added later
export function inkfallAuto(selector = 'input') {
  document.addEventListener('focusin', (e) => {
    const t = e.target;
    if (
      t instanceof HTMLInputElement
      && t.matches(selector)
      && /^(text|search|email|url|tel|)$/.test(t.getAttribute('type') || '')
      && !t._inkfall
    )
      inkfall(t);
  });
}

/* CSS (with the shared keyframes from core.js):

.is-inkfall > input { position: relative; z-index: 1; color: transparent; caret-color: transparent; }
.is-inkfall > input::selection { color: transparent; background: var(--inkfall-select, rgba(127, 127, 127, .25)); }
.inkfall-layer, .inkfall-caret { display: none; }
.is-inkfall > .inkfall-layer { display: block; position: absolute; z-index: 0; overflow: hidden; white-space: pre; pointer-events: none; }
.is-inkfall > .inkfall-layer > span { display: inline-block; white-space: pre; will-change: transform; }
.is-inkfall > .inkfall-caret { display: block; z-index: 2; opacity: 0; }
.is-inkfall > .inkfall-caret.on { opacity: 1; }
*/
