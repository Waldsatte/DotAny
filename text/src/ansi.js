// An .ans file's picture: the characters (already read from code page 437 by the Rust side, art.rs) on an 80-column
// screen, with the ANSI codes (ECMA-48) that colour them and move the cursor. Only what text art uses is read: colours
// (SGR: the 16 CGA colours, bold as the bright half, blink as the bright backgrounds the way art is drawn for, inverse,
// and the 256-colour and true-colour forms newer editors write), cursor moves, save and restore, clearing. Anything
// else is skipped; nothing in the file runs.

/** The 16 CGA colours, in ANSI's order (black, red, green, brown, blue, magenta, cyan, light grey, then the bright half).
 *  They belong to the picture, not to the window, as a photo's colours do. */
export const CGA = [
  '#000000',
  '#AA0000',
  '#00AA00',
  '#AA5500',
  '#0000AA',
  '#AA00AA',
  '#00AAAA',
  '#AAAAAA',
  '#555555',
  '#FF5555',
  '#55FF55',
  '#FFFF55',
  '#5555FF',
  '#FF55FF',
  '#55FFFF',
  '#FFFFFF',
];

// a hostile file can't grow the screen without end
const MAX_ROWS = 5000;

/** xterm's 256 colours: the 16 above, a 6×6×6 cube, 24 greys. */
function color256(n) {
  if (n < 16) return n;
  if (n < 232) {
    const v = (k) => (k ? 55 + k * 40 : 0);
    const i = n - 16;
    return rgb(v(Math.floor(i / 36)), v(Math.floor(i / 6) % 6), v(i % 6));
  }
  const g = 8 + (n - 232) * 10;
  return rgb(g, g, g);
}
const rgb = (r, g, b) =>
  '#'
  + [r, g, b]
    .map((v) =>
      Math.max(0, Math.min(255, v | 0))
        .toString(16)
        .padStart(2, '0')
    )
    .join('')
    .toUpperCase();

/**
 * The screen an .ans draws: { width, rows: [[{ ch, fg, bg }…]…] }, fg and bg a CGA index (0–15) or a #RRGGBB colour.
 * A cell nothing was written to is missing (an empty place in the row array).
 */
export function parseAnsi(text, cols = 80) {
  const rows = [];
  let x = 0,
    y = 0,
    fg = 7,
    bg = 0,
    bold = false,
    blink = false,
    inverse = false,
    saved = [0, 0],
    width = 0;
  const row = (n) => rows[n] || (rows[n] = []);
  const look = () => {
    let f = typeof fg === 'number' && bold && fg < 8 ? fg + 8 : fg,
      b = typeof bg === 'number' && blink && bg < 8 ? bg + 8 : bg;
    if (inverse) [f, b] = [b, f];
    return [f, b];
  };
  const put = (ch) => {
    if (x >= cols) {
      x = 0;
      y++;
    }
    if (y >= MAX_ROWS) return;
    const [f, b] = look();
    row(y)[x] = { ch, fg: f, bg: b };
    x++;
    if (x > width) width = x;
  };
  const sgr = (ps) => {
    if (!ps.length) ps = [0];
    for (let i = 0; i < ps.length; i++) {
      const p = ps[i];
      if (p === 0) {
        fg = 7;
        bg = 0;
        bold = blink = inverse = false;
      } else if (p === 1) bold = true;
      else if (p === 2 || p === 22) bold = false;
      else if (p === 5 || p === 6) blink = true;
      else if (p === 25) blink = false;
      else if (p === 7) inverse = true;
      else if (p === 27) inverse = false;
      else if (p >= 30 && p <= 37) fg = p - 30;
      else if (p === 39) fg = 7;
      else if (p >= 40 && p <= 47) bg = p - 40;
      else if (p === 49) bg = 0;
      else if (p >= 90 && p <= 97) fg = p - 90 + 8;
      else if (p >= 100 && p <= 107) bg = p - 100 + 8;
      else if (p === 38 || p === 48) {
        let c = null;
        if (ps[i + 1] === 5 && ps[i + 2] != null) {
          c = color256(ps[i + 2] & 255);
          i += 2;
        } else if (ps[i + 1] === 2 && ps[i + 4] != null) {
          c = rgb(ps[i + 2], ps[i + 3], ps[i + 4]);
          i += 4;
        }
        if (c != null) {
          if (p === 38) fg = c;
          else bg = c;
        }
      }
    }
  };
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === '\x1b') {
      // ESC and one more (ESC ( B and ESC ) 0 choose a character set: one more again)
      if (text[i + 1] !== '[') {
        i += /[()]/.test(text[i + 1]) ? 2 : 1;
        continue;
      }
      // ESC [ parameters final
      let j = i + 2;
      while (j < text.length && /[0-9;?=<>]/.test(text[j])) j++;
      const fin = text[j],
        raw = text.slice(i + 2, j),
        priv = /[?=<>]/.test(raw);
      const ps = raw
        .replace(/[?=<>]/g, '')
        .split(';')
        .filter((s, k, a) => s !== '' || a.length > 1)
        .map((s) => (s === '' ? 0 : parseInt(s, 10)));
      const n = (k, d = 1) => (ps[k] ? ps[k] : d);
      i = j;
      if (priv || !fin) continue;
      if (fin === 'm') sgr(ps);
      else if (fin === 'A') y = Math.max(0, y - n(0));
      else if (fin === 'B') y = Math.min(MAX_ROWS - 1, y + n(0));
      else if (fin === 'C') x = Math.min(cols - 1, x + n(0));
      else if (fin === 'D') x = Math.max(0, x - n(0));
      else if (fin === 'H' || fin === 'f') {
        y = Math.min(MAX_ROWS - 1, n(0) - 1);
        x = Math.min(cols - 1, n(1) - 1);
      } else if (fin === 's') saved = [x, y];
      else if (fin === 'u') [x, y] = saved;
      else if (fin === 'J' && ps[0] === 2) {
        rows.length = 0;
        x = 0;
        y = 0;
        width = 0;
      } else if (fin === 'K') {
        const r = rows[y];
        if (r) {
          if (!ps[0]) r.length = Math.min(r.length, x);
          else if (ps[0] === 2) r.length = 0;
          else for (let k = 0; k <= x; k++) delete r[k];
        }
      }
      continue;
    }
    if (ch === '\r') {
      x = 0;
      continue;
    }
    if (ch === '\n') {
      y++;
      x = 0;
      continue;
    }
    if (ch === '\t') {
      const to = Math.min(cols, (Math.floor(x / 8) + 1) * 8);
      while (x < to) put(' ');
      continue;
    }
    put(ch);
  }
  // nothing drawn below the last row with anything in it
  while (rows.length && !(rows[rows.length - 1] || []).some(Boolean)) rows.length--;
  return { width: Math.max(1, width), rows: Array.from(rows, (r) => r || []) };
}

/** The picture as plain text: its characters, without colours; spaces at the ends of lines dropped. */
export function ansiPlain(screen) {
  return (
    screen.rows
      .map((r) =>
        Array.from({ length: r.length }, (_, k) => (r[k] ? r[k].ch : ' '))
          .join('')
          .replace(/\s+$/, '')
      )
      .join('\n') + '\n'
  );
}

/** A colour as CSS. */
export const css = (c) => (typeof c === 'number' ? CGA[c] : c);

/** Runs of one look in a row, for drawing: [{ text, fg, bg }…]. An empty cell is a space on the picture's black. */
export function runs(r, width) {
  const out = [];
  for (let k = 0; k < width; k++) {
    const c = r[k] || { ch: ' ', fg: 7, bg: 0 };
    const last = out[out.length - 1];
    if (last && last.fg === c.fg && last.bg === c.bg) last.text += c.ch;
    else out.push({ text: c.ch, fg: c.fg, bg: c.bg });
  }
  // the black after the last character needs nothing drawn
  while (out.length && out[out.length - 1].bg === 0) {
    const last = out[out.length - 1],
      kept = last.text.replace(/ +$/, '');
    if (kept) {
      last.text = kept;
      break;
    }
    out.pop();
  }
  return out;
}
