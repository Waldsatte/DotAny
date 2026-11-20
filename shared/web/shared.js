// What every part of the page shares. `app` holds what one module lends another later on, so modules don't import in circles.
import { playTik } from './sound.js';
import { setting } from './backend.js';

export const app = {};
// asked by the Rust side before a window in the background gives back memory: never while the tool plays (app.quiet,
// the same question a button's sound asks below)
window.__DOTANY_QUIET__ = () => !!(app.quiet && app.quiet());
// the tool's own name and icons, set once by createShell before anything is drawn
export const tool = { name: 'DotAny' };
export const LUCIDE = {};
export const root = document.documentElement;
export const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
export const EASE = 'cubic-bezier(.2, .7, .2, 1)';
export const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};
export const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${LUCIDE[n]}</svg>`;
// Where a button sits at rest, without its own transform: a pressed button eases back from scale(.92), so its box read
// at the click is a little smaller, by how far the press had got. What opens under it (settings, the save card, a menu)
// is placed from this instead, on whole pixels, so it lands in the same place with every press and its words are drawn
// the same way each time.
export function restingRect(e) {
  const r = e.getBoundingClientRect(),
    c = getComputedStyle(e),
    m = new DOMMatrixReadOnly(c.transform);
  if (m.isIdentity || m.a <= 0 || m.d <= 0) return r;
  const [ox, oy] = c.transformOrigin.split(' ').map(parseFloat);
  const left = r.left - m.e - ox * (1 - m.a),
    top = r.top - m.f - oy * (1 - m.d),
    width = r.width / m.a,
    height = r.height / m.d;
  return { left, top, width, height, right: left + width, bottom: top + height };
}

// settings live on disk; the page keeps a copy so reading stays instant
const values = { ...((window.__DOTANY__ && window.__DOTANY__.settings) || {}) };
export const store = {
  get: (k) => (k in values ? values[k] : null),
  set(k, v) {
    if (v == null) delete values[k];
    else values[k] = v;
    setting(k, v);
  },
  // another window changed something
  took(k, v) {
    if (v == null) delete values[k];
    else values[k] = v;
  },
};

// one sound, off until someone turns it on
// and a button's sound never lands on top of what the tool is playing: a tool that plays sets app.quiet
export const sound = () => {
  if (store.get('dotany-sound') === 'on' && !(app.quiet && app.quiet())) playTik();
};
