// Find in a book: the words a person types and the words on the page, folded alike so that case, accents and full or
// half width don't matter (「ａ」 finds a, é finds e), and laid out as a paragraph reads, so a word split by bold or
// italics, or a Chinese sentence broken across lines, is still one word. The window's part, the bar and the jumps, is in
// main.js; this is only the matching, the same for EPUB, FB2, MOBI, TXT and a scan's text layer.

// ---------- one character, folded ----------
// lower case, then compatibility forms (full width, ligatures, circled numbers) and accents taken off; the kana's voicing
// marks stay, since か and が are two different sounds, not one letter with an accent
const folded = new Map();
const SPACE = /^[\s\x1c-\x1f]+$/;
export function fold(c) {
  const n = c.charCodeAt(0);
  if (n < 128) {
    if (n >= 65 && n <= 90) return String.fromCharCode(n + 32);
    if (n === 32 || (n >= 9 && n <= 13) || (n >= 28 && n <= 31)) return ' ';
    return n < 32 || n === 127 ? '' : c;
  }
  let f = folded.get(c);
  if (f === undefined) {
    if (SPACE.test(c)) f = ' ';
    else
      f = c
        .toLowerCase()
        .normalize('NFKD')
        .replace(/\p{M}/gu, (m) => (m === '゙' || m === '゚' ? m : ''))
        .normalize('NFC')
        .replace(/[\p{Cc}\p{Cf}]/gu, '');
    if (f && SPACE.test(f)) f = ' ';
    folded.set(c, f);
  }
  return f;
}
// Chinese and Japanese have no spaces between words: a line break or space beside one is the layout's, not the text's
const CJK = /[\p{sc=Han}\p{sc=Hiragana}\p{sc=Katakana}　-〿ー]/u;
// paragraphs never run into each other: a match stops at the end of one
const SEP = '\n';

// a string folded the same way, with where each folded character came from (its offset in the original) when asked
function folder(wantMap) {
  let out = '',
    pend = -1,
    lastCjk = false;
  const map = wantMap ? [] : null;
  return {
    push(s, base) {
      for (let i = 0; i < s.length; i++) {
        let c = s[i];
        const u = s.charCodeAt(i);
        if (u >= 0xd800 && u < 0xdc00 && i + 1 < s.length) c = s.slice(i, i + 2);
        const at = base + i;
        i += c.length - 1;
        const f = fold(c);
        if (!f) continue;
        if (f === ' ') {
          if (pend < 0 && out && out[out.length - 1] !== SEP) pend = at;
          continue;
        }
        if (pend >= 0) {
          if (!lastCjk && !CJK.test(f)) {
            out += ' ';
            if (map) map.push(pend);
          }
          pend = -1;
        }
        out += f;
        if (map) for (let k = 0; k < f.length; k++) map.push(at);
        lastCjk = CJK.test(f[f.length - 1]);
      }
    },
    space(at) {
      if (pend < 0 && out && out[out.length - 1] !== SEP) pend = at;
    },
    stop(at) {
      pend = -1;
      if (out && out[out.length - 1] !== SEP) {
        out += SEP;
        if (map) map.push(at);
      }
      lastCjk = false;
    },
    get text() {
      return out;
    },
    map,
  };
}

/** What a person typed, folded: '' when there's nothing to look for. */
export function foldQuery(s) {
  const f = folder(false);
  f.push(s, 0);
  return f.text.replace(/\n/g, '');
}

// ---------- a chapter, as a reader reads it ----------
const BLOCK = new Set([
  'P',
  'H1',
  'H2',
  'H3',
  'H4',
  'H5',
  'H6',
  'LI',
  'BLOCKQUOTE',
  'TD',
  'TH',
  'TR',
  'DIV',
  'SECTION',
  'ARTICLE',
  'ASIDE',
  'FIGURE',
  'FIGCAPTION',
  'PRE',
  'DT',
  'DD',
  'DL',
  'UL',
  'OL',
  'TABLE',
  'HEADER',
  'FOOTER',
  'HR',
  'NAV',
]);
const blockOf = (n, top) => {
  for (let e = n.parentNode; e && e !== top; e = e.parentNode) if (BLOCK.has(e.nodeName.toUpperCase())) return e;
  return top;
};
const unseen = (n, top) => {
  for (let e = n.parentNode; e && e !== top; e = e.parentNode) {
    const t = e.nodeName.toUpperCase();
    if (t === 'RT' || t === 'RP') return true;
  }
  return false;
};
/** A chapter's text, folded. With wantMap, also its text nodes, where each starts, and where each folded character came
 *  from, so a match can become a Range on the page. The page and the search read the same HTML, so they agree. */
export function foldChapter(top, wantMap = false) {
  const f = folder(wantMap),
    nodes = wantMap ? [] : null,
    starts = wantMap ? [] : null;
  const w = (top.ownerDocument || document).createTreeWalker(top, 5 /* SHOW_ELEMENT | SHOW_TEXT */);
  let g = 0,
    block = null;
  for (let n = w.nextNode(); n; n = w.nextNode()) {
    if (n.nodeType === 1) {
      if (n.nodeName.toUpperCase() === 'BR') f.space(g);
      continue;
    }
    const s = n.nodeValue;
    if (!s || unseen(n, top)) continue;
    const b = blockOf(n, top);
    if (b !== block) {
      f.stop(g);
      block = b;
    }
    if (wantMap) {
      nodes.push(n);
      starts.push(g);
    }
    f.push(s, g);
    g += s.length;
  }
  return { text: f.text, map: f.map, nodes, starts };
}
/** Every place the folded query is in the folded text, one after another, never overlapping. */
export function matches(text, q) {
  const out = [];
  if (!q) return out;
  for (let at = text.indexOf(q); at >= 0; at = text.indexOf(q, at + q.length)) out.push(at);
  return out;
}
/** A match in a chapter folded with its map, as a Range over the page's own text. */
export function rangeOf(m, at, len) {
  const a = m.map[at],
    z = m.map[at + len - 1];
  if (a == null || z == null) return null;
  const where = (o) => {
    let lo = 0,
      hi = m.starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (m.starts[mid] <= o) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  };
  const i = where(a),
    j = where(z),
    nz = m.nodes[j],
    oz = z - m.starts[j],
    u = nz.nodeValue.charCodeAt(oz);
  const r = (m.nodes[0].ownerDocument || document).createRange();
  r.setStart(m.nodes[i], a - m.starts[i]);
  r.setEnd(nz, Math.min(nz.nodeValue.length, oz + (u >= 0xd800 && u < 0xdc00 ? 2 : 1)));
  return r;
}

// ---------- a scan's text layer: words with boxes ----------
/** A DjVu page's words ({ x, y, width, height, text }, y from the bottom), folded, with each word's place in the text. */
export function foldWords(zones) {
  const f = folder(true),
    ends = [];
  let g = 0;
  for (const z of zones) {
    f.push(z.text, g);
    g += z.text.length;
    ends.push(g);
  }
  return { text: f.text, map: f.map, zones, ends };
}
/** A match on a scan, as boxes in the page's own pixels from the top left: a part of a word is a part of its box. */
export function boxesOf(p, at, len, height) {
  const a = p.map[at],
    z = p.map[at + len - 1] + 1,
    out = [];
  let start = 0;
  p.zones.forEach((w, i) => {
    const end = p.ends[i],
      from = Math.max(a, start),
      to = Math.min(z, end);
    if (from < to) {
      const L = Math.max(1, w.text.replace(/\s+$/, '').length),
        x0 = Math.min(L, from - start),
        x1 = Math.min(L, to - start);
      out.push([w.x + (w.width * x0) / L, height - w.y - w.height, Math.max(1, (w.width * (x1 - x0)) / L), w.height]);
    }
    start = end;
  });
  return out;
}
