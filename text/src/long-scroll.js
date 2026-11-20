// The read-only long scroll. Only the lines on screen exist in the page; Rust reads them from disk as they are needed.
// The position is a line number plus how far into that line, so scrolling stays exact at any file size. Browsers cap an
// element's height near 33 million pixels, so the scrollbar is proportional, while the wheel moves real pixels.
import { long, on } from './backend.js';
import { reduce } from '@dotany/web/shared.js';
import { modKey, macEdge } from '@dotany/web/keys.js';
import { mac, wheelSource } from '@dotany/web/gestures.js';

const fromTrackpad = mac ? wheelSource() : null;
const CHUNK = 200,
  KEEP = 80,
  ABOVE = 10,
  BELOW = 24;
const LEVEL = /\b(ERROR|WARN|INFO|DEBUG)\b/g;
const MAX_SPACER = 16_000_000;
const WHEEL_TAU = 55;

export function createLong({ el, host, onMove, onCount, onFindCount }) {
  const scroll = el('div', 'ls');
  scroll.tabIndex = 0;
  const spacer = el('div', 'ls-spacer'),
    viewEl = el('div', 'ls-view'),
    rowsEl = el('div', 'ls-rows');
  viewEl.appendChild(rowsEl);
  scroll.append(spacer, viewEl);
  host.appendChild(scroll);

  let total = 1,
    top = 0,
    lineH = 23.1,
    wrap = false,
    follow = false,
    active = false,
    gen = 0;
  let chunks = new Map(),
    waiting = new Set(),
    rowsDom = new Map(),
    heights = new Map();
  let lastSet = -1,
    raf = 0,
    wheelRaf = 0,
    pending = 0,
    lastT = 0,
    widest = 0,
    findCur = null;

  // the glass at the top grows when a notice opens, and the words move down with it
  const glass = document.querySelector('.chrome');
  const padTop = () => glass.offsetHeight + 16;
  const padBottom = () =>
    (parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--status-h')) || 30) + 16;
  const heightOf = (row) => heights.get(row) || lineH;
  const visibleRows = () => (scroll.clientHeight - padTop() - padBottom()) / lineH;
  const maxTop = () => Math.max(0, total - Math.max(1, Math.floor(visibleRows())));

  function lineOf(row) {
    const c = chunks.get(Math.floor(row / CHUNK));
    return c ? c[row % CHUNK] : undefined;
  }
  function need(row) {
    const i = Math.floor(row / CHUNK);
    if (chunks.has(i) || waiting.has(i)) return;
    waiting.add(i);
    const g = gen;
    long
      .lines(i * CHUNK, CHUNK)
      .then((lines) => {
        waiting.delete(i);
        if (g !== gen) return;
        chunks.set(i, lines);
        if (chunks.size > KEEP) {
          // forget what is furthest from the view
          const here = Math.floor(top / CHUNK);
          let far = -1,
            dist = -1;
          for (const k of chunks.keys())
            if (Math.abs(k - here) > dist) {
              dist = Math.abs(k - here);
              far = k;
            }
          chunks.delete(far);
        }
        draw();
      })
      .catch(() => {
        waiting.delete(i);
      });
  }

  // the words of one line: level names and find matches stand out, the same way they do in the code view
  function fill(text, line) {
    const spans = [];
    if (line) {
      for (const m of line.text.matchAll(LEVEL))
        spans.push([m.index, m.index + m[0].length, 'log-' + m[0].toLowerCase()]);
      line.marks.forEach(([a, b], nth) =>
        spans.push([
          a,
          b,
          findCur && findCur.line === line.no && findCur.nth === nth
            ? 'cm-searchMatch cm-searchMatch-selected'
            : 'cm-searchMatch',
        ])
      );
    }
    text.textContent = '';
    if (!line) return;
    if (!spans.length) {
      text.textContent = line.text;
      return;
    }
    spans.sort((x, y) => x[0] - y[0]);
    let at = 0;
    for (const [a, b, cls] of spans) {
      if (a < at) continue;
      if (a > at) text.appendChild(document.createTextNode(line.text.slice(at, a)));
      const s = el('span', cls, line.text.slice(a, b));
      text.appendChild(s);
      at = b;
    }
    if (at < line.text.length) text.appendChild(document.createTextNode(line.text.slice(at)));
  }
  function makeRow(row) {
    const d = el('div', 'ls-row'),
      no = el('span', 'ls-no'),
      text = el('span', 'cm-line ls-t');
    d.append(no, text);
    d._row = row;
    return d;
  }
  function paintRow(d, row) {
    const line = lineOf(row);
    if (d._line === line && line) return;
    d._line = line;
    d.firstChild.textContent = line ? (line.no + 1).toLocaleString('en-US').replace(/,/g, '') : '';
    fill(d.lastChild, line);
  }

  function draw() {
    if (!active) return;
    raf = 0;
    const first = Math.max(0, Math.floor(top) - ABOVE);
    const count = Math.ceil(scroll.clientHeight / lineH) + ABOVE + BELOW;
    const last = Math.min(total - 1, first + count);
    for (let r = first; r <= last; r += 1) if (r % CHUNK === 0 || r === first) need(r);
    for (const [row, d] of rowsDom)
      if (row < first || row > last) {
        d.remove();
        rowsDom.delete(row);
      }
    let before = null;
    for (let r = last; r >= first; r--) {
      let d = rowsDom.get(r);
      if (!d) {
        d = makeRow(r);
        rowsDom.set(r, d);
        rowsEl.insertBefore(d, before);
      }
      paintRow(d, r);
      before = d;
    }
    if (wrap) for (const [row, d] of rowsDom) heights.set(row, d.offsetHeight || lineH);
    else {
      // long lines stretch the scrollbar as they come into view
      const w = rowsEl.scrollWidth;
      if (w > widest) {
        widest = w;
        spacer.style.width = w + 'px';
      }
    }
    place();
  }
  function place() {
    const whole = Math.floor(top),
      first = rowsEl.firstChild ? rowsEl.firstChild._row : whole;
    let y = padTop();
    for (let r = first; r < whole; r++) y -= heightOf(r);
    y -= (top - whole) * heightOf(whole);
    rowsEl.style.transform = `translate(${-scroll.scrollLeft}px, ${y}px)`;
    rowsEl.style.setProperty('--sx', scroll.scrollLeft + 'px');
    syncBar();
    onMove(whole, total);
  }
  function syncBar() {
    const room = scroll.scrollHeight - scroll.clientHeight,
      mt = maxTop();
    const want = mt > 0 ? (top / mt) * room : 0;
    if (Math.abs(want - scroll.scrollTop) > 0.5) {
      scroll.scrollTop = want;
      lastSet = scroll.scrollTop;
    }
  }
  const request = () => {
    if (!raf) raf = requestAnimationFrame(draw);
  };

  function moveBy(px) {
    let whole = Math.floor(top),
      frac = top - whole;
    let d = px;
    while (d > 0) {
      const h = heightOf(whole),
        left = (1 - frac) * h;
      if (d < left) {
        frac += d / h;
        d = 0;
      } else {
        d -= left;
        whole++;
        frac = 0;
        if (whole >= total) break;
      }
    }
    while (d < 0) {
      const h = heightOf(whole),
        here = frac * h;
      if (-d <= here) {
        frac += d / h;
        d = 0;
      } else {
        d += here;
        whole--;
        frac = 1;
        if (whole < 0) {
          whole = 0;
          frac = 0;
          break;
        }
      }
    }
    top = Math.min(maxTop(), Math.max(0, whole + frac));
    request();
  }
  function wheelFrame(now) {
    wheelRaf = 0;
    const dt = Math.min(64, now - lastT);
    lastT = now;
    const step = reduce ? pending : pending * (1 - Math.exp(-dt / WHEEL_TAU));
    const use = Math.abs(pending - step) < 0.5 ? pending : step;
    pending -= use;
    moveBy(use);
    if (pending) wheelRaf = requestAnimationFrame(wheelFrame);
  }
  scroll.addEventListener(
    'wheel',
    (e) => {
      if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY) || e.ctrlKey) return;
      e.preventDefault();
      // the Mac's trackpad already glides, with the system's momentum: the lines follow the fingers at once
      if (fromTrackpad && fromTrackpad(e) === 'trackpad') {
        pending = 0;
        moveBy(e.deltaY);
        return;
      }
      pending += e.deltaY * (e.deltaMode === 1 ? lineH : e.deltaMode === 2 ? scroll.clientHeight : 1);
      if (!wheelRaf) {
        lastT = performance.now();
        wheelRaf = requestAnimationFrame(wheelFrame);
      }
    },
    { passive: false }
  );
  // dragging the scrollbar: a jump along the whole file
  scroll.addEventListener(
    'scroll',
    () => {
      if (Math.abs(scroll.scrollTop - lastSet) > 1) {
        const room = scroll.scrollHeight - scroll.clientHeight;
        if (room > 0) {
          pending = 0;
          top = (scroll.scrollTop / room) * maxTop();
          lastSet = scroll.scrollTop;
        }
      }
      request();
    },
    { passive: true }
  );
  scroll.addEventListener('keydown', (e) => {
    const page = scroll.clientHeight - padTop() - padBottom();
    const go = {
      ArrowDown: () => moveBy(lineH),
      ArrowUp: () => moveBy(-lineH),
      PageDown: () => moveBy(page),
      PageUp: () => moveBy(-page),
      Home: () => jump(0),
      End: () => jump(total),
    }[macEdge(e) === 'start' ? 'Home' : macEdge(e) === 'end' ? 'End' : e.key];
    // the Mac's ⌘↑ ⌘↓ are its Home and End (macEdge), and fn ← → still make those
    if ((go && !modKey(e)) || (go && (e.key === 'Home' || e.key === 'End' || macEdge(e)))) {
      e.preventDefault();
      go();
    }
  });
  const onResize = () => {
    if (active) {
      if (wrap) heights.clear();
      fitSpacer();
      request();
    }
  };
  new ResizeObserver(onResize).observe(scroll);
  new ResizeObserver(onResize).observe(glass);

  function fitSpacer() {
    spacer.style.height = Math.min(MAX_SPACER, total * lineH + padTop() + padBottom()) + 'px';
  }
  function jump(row, center) {
    pending = 0;
    top = Math.max(0, Math.min(maxTop(), center ? row - Math.floor(visibleRows() / 2) : row));
    request();
  }
  function clearRows() {
    gen++;
    chunks = new Map();
    waiting = new Set();
    heights = new Map();
    widest = 0;
    rowsDom.forEach((d) => d.remove());
    rowsDom = new Map();
    spacer.style.width = '';
  }
  function taken(p) {
    if (!active) return;
    const grew = p.rows !== total;
    const hadEnd = total > 0 && top >= maxTop() - 1;
    if (p.rows < total) clearRows();
    // a growing last chunk must be read again
    if (grew) {
      const i = Math.floor((total - 1) / CHUNK);
      chunks.delete(i);
    }
    total = Math.max(1, p.rows);
    onCount(p);
    fitSpacer();
    if (follow && (grew || hadEnd)) top = maxTop();
    request();
  }
  on('long-progress', taken);
  on('long-grew', () => {
    long.state().then(taken);
  });
  on('long-reset', () => {
    if (!active) return;
    clearRows();
    total = 1;
    top = 0;
    long.state().then(taken);
  });
  on('long-find', (s) => {
    if (!active) return;
    rerender();
    onFindCount(s);
  });

  // find matches come with the lines, so what is on screen must be read again
  function rerender() {
    gen++;
    chunks = new Map();
    waiting = new Set();
    rowsDom.forEach((d) => {
      d._line = undefined;
    });
    request();
  }

  return {
    dom: scroll,
    async open(startFollow) {
      active = true;
      clearRows();
      top = 0;
      pending = 0;
      total = 1;
      follow = !!startFollow;
      lineH = parseFloat(getComputedStyle(scroll).lineHeight) || lineH;
      scroll.hidden = false;
      fitSpacer();
      draw();
      taken(await long.state());
    },
    close() {
      active = false;
      follow = false;
      pending = 0;
      scroll.hidden = true;
      clearRows();
    },
    focus() {
      scroll.focus({ preventScroll: true });
    },
    setWrap(on) {
      wrap = on;
      scroll.classList.toggle('wrap', on);
      heights.clear();
      clearLayout();
    },
    setFollow(on) {
      follow = on;
      if (on) jump(total);
    },
    setFilter(level) {
      top = 0;
      return long.filter(level).then(() => {
        rerender();
      });
    },
    jump,
    top: () => Math.floor(top),
    total: () => total,
    setCurrent(hit) {
      findCur = hit;
      rowsDom.forEach((d) => {
        d._line = undefined;
      });
      request();
    },
    refresh() {
      lineH = parseFloat(getComputedStyle(scroll).lineHeight) || lineH;
      fitSpacer();
      request();
    },
    rerender,
  };

  function clearLayout() {
    widest = 0;
    spacer.style.width = '';
    rowsDom.forEach((d) => {
      d._line = undefined;
    });
    request();
  }
}
