// What a sheet is: a cell is a string (as in the file), { n, fmt } for a number,
// or { f, c, fmt } for a formula and the result the file came with. Text tables are kept exactly as written; formulas are
// the everyday ones, and anything else keeps the result already in the file.

// ---------- text tables: nothing is converted. "007" stays "007", a long ID stays a long ID ----------
export function sniffDelimiter(text, name) {
  if (/\.tsv$/i.test(name)) return '\t';
  // the first rows, read the way parseDelimited reads them: a separator inside quotes ("Smith, John") isn't one. The
  // separator is the one that splits the most rows into the same number of fields as the first, then the one that
  // makes the most fields; a file with none of them is comma-separated
  const sample = text.slice(0, 64 * 1024),
    whole = sample.length === text.length;
  const perRow = (d) => {
    const out = [];
    let n = 0,
      q = false,
      start = true,
      i = 0;
    for (; i < sample.length && out.length < 20; i++) {
      const c = sample[i];
      if (q) {
        if (c === '"') {
          if (sample[i + 1] === '"') i++;
          else q = false;
        }
        continue;
      }
      if (c === '"' && start) {
        q = true;
        start = false;
        continue;
      }
      if (c === d) {
        n++;
        start = true;
        continue;
      }
      if (c === '\n' || c === '\r') {
        if (c === '\r' && sample[i + 1] === '\n') i++;
        out.push(n);
        n = 0;
        start = true;
        continue;
      }
      start = false;
    }
    if (whole && i >= sample.length && !q && (n || !start)) out.push(n);
    return out;
  };
  let best = ',',
    bestKey = [0, 0];
  for (const d of [',', '\t', ';']) {
    const rows = perRow(d),
      first = rows[0] || 0;
    if (!first) continue;
    const key = [rows.filter((n) => n === first).length, first];
    if (key[0] > bestKey[0] || (key[0] === bestKey[0] && key[1] > bestKey[1])) {
      best = d;
      bestKey = key;
    }
  }
  return best;
}
/**
 * The rows, and for every row how it was written: `q[i]` is a field's own text when it was quoted (so it can go back the
 * same way), `eol` the line ending after it. A row nobody touched is written back from these, byte for byte.
 */
export function parseDelimited(text, d = ',') {
  const rows = [],
    raw = [];
  let row = [],
    cell = '',
    q = false,
    i = 0,
    quoted = false,
    start = 0,
    quotes = null;
  const n = text.length;
  const endCell = (end) => {
    if (quoted) (quotes || (quotes = []))[row.length] = text.slice(start, end);
    row.push(cell);
    cell = '';
    quoted = false;
  };
  const endRow = (eol) => {
    rows.push(row);
    raw.push({ q: quotes, eol, orig: row.slice() });
    row = [];
    quotes = null;
  };
  while (i < n) {
    const c = text[i];
    if (q) {
      if (c === '"') {
        if (text[i + 1] === '"') {
          cell += '"';
          i += 2;
          continue;
        }
        q = false;
        i++;
        continue;
      }
      cell += c;
      i++;
      continue;
    }
    if (c === '"' && !cell && !quoted) {
      q = true;
      quoted = true;
      i++;
      continue;
    }
    if (c === d) {
      endCell(i);
      i++;
      start = i;
      continue;
    }
    if (c === '\r' && text[i + 1] === '\n') {
      endCell(i);
      endRow('\r\n');
      i += 2;
      start = i;
      continue;
    }
    if (c === '\n' || c === '\r') {
      endCell(i);
      endRow(c);
      i++;
      start = i;
      continue;
    }
    cell += c;
    i++;
  }
  if (cell || quoted || row.length) {
    endCell(n);
    endRow('');
  }
  return { rows, raw };
}
export const lineEndOf = (text) => {
  const s = text.slice(0, 8000);
  return /\r\n/.test(s) ? '\r\n' : /\n/.test(s) ? '\n' : /\r/.test(s) ? '\r' : '\r\n';
};
const needsQuote = (s, d) => s.includes(d) || s.includes('"') || s.includes('\n') || s.includes('\r');
export const quoteField = (s, d) => (needsQuote(s, d) ? '"' + s.replace(/"/g, '""') + '"' : s);
/**
 * Rows back to text. `raw` is what parseDelimited gave for the rows that came from the file (undefined for new ones):
 * a row that still holds what it held is its old line again; in a changed row only the changed fields are written anew.
 */
export function writeDelimited(rows, d = ',', eol = '\r\n', raw = null, finalEol = true) {
  const out = [];
  rows.forEach((row, i) => {
    const r = raw && raw[i],
      last = i === rows.length - 1;
    let line;
    if (r) {
      const same = row.length === r.orig.length && row.every((v, j) => v === r.orig[j]);
      line = row
        .map((v, j) => (j < r.orig.length && v === r.orig[j] ? (r.q && r.q[j] != null ? r.q[j] : v) : quoteField(v, d)))
        .join(d);
      void same;
    } else line = row.map((v) => quoteField(v, d)).join(d);
    out.push(line, last ? (r ? r.eol : finalEol ? eol : '') : (r && r.eol) || eol);
  });
  return out.join('');
}

// ---------- names and numbers ----------
export const colNo = (ref) => {
  let n = 0;
  for (const ch of ref) {
    const c = ch.charCodeAt(0);
    if (c < 65 || c > 90) break;
    n = n * 26 + c - 64;
  }
  return n - 1;
};
export const colName = (i) => {
  let s = '';
  i++;
  while (i > 0) {
    const m = (i - 1) % 26;
    s = String.fromCharCode(65 + m) + s;
    i = (i - m - 1) / 26;
  }
  return s;
};
const pad = (n) => String(n).padStart(2, '0');
// a workbook may count its days from 1904 (old Mac files); `epoch.base` is the day number of 1970-01-01 in its count
export const epoch = { base: 25569 };
const dayParts = (n) => {
  const d = new Date(Math.round((n - epoch.base) * 86400000));
  return { y: d.getUTCFullYear(), m: d.getUTCMonth() + 1, d: d.getUTCDate(), wd: d.getUTCDay() };
};
export function formatNumber(n, fmt) {
  if (fmt === '%') return (n * 100).toLocaleString('en-US', { maximumFractionDigits: 2 }) + '%';
  if (fmt === 'date') {
    const p = dayParts(n);
    return `${p.y}-${pad(p.m)}-${pad(p.d)}`;
  }
  if (fmt === ',') return n.toLocaleString('en-US', { maximumFractionDigits: 2 });
  return Number.isInteger(n) ? String(n) : String(+n.toFixed(10));
}
// a string counts as a number for adding up and aligning, but it is never rewritten: zero-led codes and long IDs aren't numbers at all
export const looksNumber = (s) => /^-?(0|[1-9]\d{0,14})(\.\d+)?$/.test(s);
const isoDay = (s) => {
  const m = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(s);
  return m ? serial(+m[1], +m[2], +m[3]) : null;
};
export function serial(y, m, d) {
  return Date.UTC(y, m - 1, d) / 86400000 + epoch.base;
}

// ---------- formulas: the everyday ones. Anything else keeps the result already in the file ----------
export class NotSupported extends Error {}
// `code` is what a spreadsheet file calls the error; the window shows its own word for it
export class FormulaError extends Error {
  constructor(code = '#VALUE!') {
    super(code);
    this.code = code;
  }
}
export const ERRORS = ['#NULL!', '#DIV/0!', '#VALUE!', '#REF!', '#NAME?', '#NUM!', '#N/A'];
const TOKEN =
  /\s*(?:(\d+(?:\.\d+)?(?:[eE][-+]?\d+)?)|("(?:[^"]|"")*")|(#(?:NULL!|DIV\/0!|VALUE!|REF!|NAME\?|NUM!|N\/A))|((?:'(?:[^']|'')+'|[^\s!'"(),+\-*/&=<>:;^%{}]+)!)?(\$?[A-Za-z]{1,3}\$?\d+)(?::(\$?[A-Za-z]{1,3}\$?\d+))?(?![A-Za-z0-9_(.])|([A-Za-z_\\][A-Za-z0-9_.]*)(\()?|(<=|>=|<>|[-+*/^&=<>(),%]))/y;
function tokens(src) {
  const out = [];
  let at = 0;
  while (at < src.length) {
    TOKEN.lastIndex = at;
    const m = TOKEN.exec(src);
    if (!m) {
      if (!src.slice(at).trim()) break;
      throw new NotSupported();
    }
    at = TOKEN.lastIndex;
    if (m[1] != null) out.push({ t: 'num', v: +m[1] });
    else if (m[2] != null) out.push({ t: 'str', v: m[2].slice(1, -1).replace(/""/g, '"') });
    else if (m[3] != null) out.push({ t: 'err', v: m[3] });
    else if (m[5] != null)
      out.push({
        t: 'ref',
        sheet: m[4] ? m[4].slice(0, -1).replace(/^'|'$/g, '').replace(/''/g, "'") : null,
        a: m[5].replace(/\$/g, '').toUpperCase(),
        b: m[6] ? m[6].replace(/\$/g, '').toUpperCase() : null,
      });
    else if (m[7] != null)
      out.push(
        m[8]
          ? {
              t: 'fn',
              v: m[7]
                .toUpperCase()
                .replace(/^_XLFN\.|^_XLWS\./, '')
                .replace(/^_XLFN\./, ''),
            }
          : { t: 'name', v: m[7] }
      );
    else out.push({ t: 'op', v: m[9] });
  }
  return out;
}
// ---------- parsing to a tree, so IF and IFERROR only work out the side they take ----------
export function parse(source) {
  const toks = tokens(source.replace(/^=/, ''));
  let i = 0;
  const peek = () => toks[i],
    take = () => toks[i++],
    isOp = (v) => peek() && peek().t === 'op' && peek().v === v;
  const need = (v) => {
    if (!isOp(v)) throw new NotSupported();
    take();
  };
  const bin = (next, ops) => () => {
    let a = next();
    while (peek() && peek().t === 'op' && ops.includes(peek().v)) {
      const op = take().v;
      a = { t: 'bin', op, a, b: next() };
    }
    return a;
  };
  const unary = () => {
    if (isOp('-') || isOp('+')) {
      const op = take().v;
      return { t: 'neg', op, a: unary() };
    }
    let a = atom();
    while (isOp('%')) {
      take();
      a = { t: 'pct', a };
    }
    return a;
  };
  const power = bin(unary, ['^']),
    term = bin(power, ['*', '/']),
    sum = bin(term, ['+', '-']),
    concat = bin(sum, ['&']),
    compare = bin(concat, ['=', '<>', '<', '>', '<=', '>=']);
  function atom() {
    const k = take();
    if (!k) throw new NotSupported();
    if (k.t === 'num' || k.t === 'str' || k.t === 'err' || k.t === 'ref') return k;
    if (k.t === 'name') {
      const u = k.v.toUpperCase();
      return u === 'TRUE' || u === 'FALSE' ? { t: 'bool', v: u === 'TRUE' } : k;
    }
    if (k.t === 'op' && k.v === '(') {
      const v = compare();
      need(')');
      return v;
    }
    if (k.t === 'fn') {
      const args = [];
      if (!isOp(')')) {
        for (;;) {
          args.push(isOp(',') || isOp(')') ? { t: 'blank' } : compare());
          if (!isOp(',')) break;
          take();
          if (isOp(')')) args.push({ t: 'blank' });
        }
      }
      need(')');
      return { t: 'fn', v: k.v, args };
    }
    throw new NotSupported();
  }
  const tree = compare();
  if (i < toks.length) throw new NotSupported();
  return tree;
}

// ---------- values ----------
// a range is { rows: [[…]] }; a cell that holds an error carries it as a FormulaError in the range
const isRange = (v) => v != null && typeof v === 'object' && Array.isArray(v.rows);
const cells = (v) => (isRange(v) ? v.rows.flat() : [v]);
const scalar = (v) => {
  if (!isRange(v)) return v;
  if (v.rows.length === 1 && v.rows[0].length === 1) return v.rows[0][0];
  throw new FormulaError('#VALUE!');
};
const check = (v) => {
  if (v instanceof FormulaError) throw v;
  return v;
};
const num = (v) => {
  v = check(scalar(v));
  if (typeof v === 'number') return v;
  if (v === '' || v == null) return 0;
  if (typeof v === 'boolean') return v ? 1 : 0;
  const s = String(v).trim();
  if (looksNumber(s) || /^-?\d*\.?\d+(e[-+]?\d+)?$/i.test(s)) return +s;
  if (/^-?\d+(\.\d+)?%$/.test(s)) return parseFloat(s) / 100;
  const day = isoDay(s);
  if (day != null) return day;
  throw new FormulaError('#VALUE!');
};
const str = (v) => {
  v = check(scalar(v));
  if (typeof v === 'boolean') return v ? 'TRUE' : 'FALSE';
  if (typeof v === 'number') return formatNumber(v, '');
  return v == null ? '' : String(v);
};
const bool = (v) => {
  v = check(scalar(v));
  if (typeof v === 'string') {
    const u = v.toUpperCase();
    if (u === 'TRUE') return true;
    if (u === 'FALSE') return false;
    if (v === '') return false;
    throw new FormulaError('#VALUE!');
  }
  return !!num(v);
};
// in a range, numbers are what adds up: words, blanks and TRUE are passed over, as every spreadsheet does
const numsOf = (args) => {
  const out = [];
  for (const a of args)
    for (const v of cells(a)) {
      check(v);
      if (typeof v === 'number') out.push(v);
      else if (typeof v === 'string' && looksNumber(v)) out.push(+v);
      else if (!isRange(a) && typeof v === 'boolean') out.push(+v);
    }
  return out;
};
const total = (a) => a.reduce((x, y) => x + y, 0);
const roundTo = (x, k, how) => {
  const f = 10 ** k,
    y = x * f,
    r =
      how === 'up'
        ? Math.sign(y) * Math.ceil(Math.abs(y) - 1e-9)
        : how === 'down'
          ? Math.sign(y) * Math.floor(Math.abs(y) + 1e-9)
          : Math.sign(y) * Math.round(Math.abs(y) + 1e-9);
  return r / f;
};
const same = (a, b) =>
  typeof a === 'string' && typeof b === 'string'
    ? a.toLowerCase() === b.toLowerCase()
    : a === b || (a === '' && b === 0) || (a === 0 && b === '');
const order = (a, b) => {
  const rank = (v) => (typeof v === 'number' || v === '' ? 0 : typeof v === 'string' ? 1 : 2);
  if (rank(a) !== rank(b)) return rank(a) - rank(b);
  if (typeof a === 'string') return a.toLowerCase() < b.toLowerCase() ? -1 : a.toLowerCase() > b.toLowerCase() ? 1 : 0;
  return (+a || 0) - (+b || 0);
};
// "East", ">10", "<>x", "=": what COUNTIF and its kin test each cell against
function criterion(c) {
  c = check(scalar(c));
  if (typeof c !== 'string') return (v) => same(typeof v === 'string' && looksNumber(v) ? +v : v, c);
  const m = /^(<=|>=|<>|<|>|=)?([\s\S]*)$/.exec(c),
    op = m[1] || '=',
    rest = m[2];
  const n = rest !== '' && !isNaN(+rest) ? +rest : null;
  const like = /[*?]/.test(rest)
    ? new RegExp(
        '^'
          + rest
            .replace(/[.+^${}()|[\]\\]/g, '\\$&')
            .replace(/~\*/g, '\u0001')
            .replace(/~\?/g, '\u0002')
            .replace(/\*/g, '[\\s\\S]*')
            .replace(/\?/g, '.')
            .replace(/\u0001/g, '\\*')
            .replace(/\u0002/g, '\\?')
          + '$',
        'i'
      )
    : null;
  return (v) => {
    if (v instanceof FormulaError) return false;
    const vn = typeof v === 'number' ? v : typeof v === 'string' && looksNumber(v) ? +v : null;
    if (op === '=' || op === '<>') {
      let hit;
      if (rest === '') hit = v === '' || v == null;
      else if (n != null) hit = vn === n;
      else if (like) hit = typeof v === 'string' && like.test(v);
      else hit = typeof v === 'string' && v.toLowerCase() === rest.toLowerCase();
      return op === '=' ? hit : !hit;
    }
    if (n != null) {
      if (vn == null) return false;
      return op === '<' ? vn < n : op === '>' ? vn > n : op === '<=' ? vn <= n : vn >= n;
    }
    if (typeof v !== 'string' || v === '') return false;
    const d = order(v, rest);
    return op === '<' ? d < 0 : op === '>' ? d > 0 : op === '<=' ? d <= 0 : d >= 0;
  };
}
// SUMIFS and the like: pairs of a range and its test, every pair true for a cell to count
function ifs(pairs) {
  const tests = [];
  for (let i = 0; i < pairs.length; i += 2) {
    const r = pairs[i];
    if (!isRange(r)) throw new FormulaError('#VALUE!');
    tests.push([r.rows.flat(), criterion(pairs[i + 1])]);
  }
  const n = tests.length ? tests[0][0].length : 0;
  if (tests.some((t) => t[0].length !== n)) throw new FormulaError('#VALUE!');
  const hits = [];
  for (let i = 0; i < n; i++) if (tests.every(([vals, ok]) => ok(vals[i]))) hits.push(i);
  return hits;
}
const pick = (range, hits) => {
  const vals = range.rows.flat();
  return hits
    .map((i) => vals[i])
    .filter((v) => {
      check(v);
      return typeof v === 'number' || (typeof v === 'string' && looksNumber(v));
    })
    .map(Number);
};
function lookup(key, list, exact) {
  key = check(scalar(key));
  if (exact) {
    const ok =
      typeof key === 'string'
        ? criterion(key.replace(/^([<>=])/, '=$1'))
        : (v) => same(typeof v === 'string' && looksNumber(v) ? +v : v, key);
    const i = list.findIndex(ok);
    if (i < 0) throw new FormulaError('#N/A');
    return i;
  }
  // sorted, the largest that's not more than the key
  let at = -1;
  for (let i = 0; i < list.length; i++) {
    const v = list[i];
    if (v === '' || v instanceof FormulaError) continue;
    if (order(v, key) <= 0) at = i;
    else break;
  }
  if (at < 0) throw new FormulaError('#N/A');
  return at;
}
const day = (n) => dayParts(Math.floor(num(n)));
function text(v, f) {
  const n = num(v),
    fmt = str(f);
  const date = /^[ymd/\-.\s]+$/i.test(fmt) && /[ymd]/i.test(fmt);
  if (date) {
    const p = day(n);
    return fmt.replace(/yyyy|yy|mm|m|dd|d/gi, (t) => {
      const k = t.toLowerCase();
      return k === 'yyyy'
        ? String(p.y)
        : k === 'yy'
          ? pad(p.y % 100)
          : k === 'mm'
            ? pad(p.m)
            : k === 'm'
              ? String(p.m)
              : k === 'dd'
                ? pad(p.d)
                : String(p.d);
    });
  }
  const m = /^(#,##)?0(\.(0+))?(%)?$/.exec(fmt);
  if (!m) throw new NotSupported();
  const places = m[3] ? m[3].length : 0,
    x = m[4] ? n * 100 : n;
  const s = m[1]
    ? roundTo(x, places).toLocaleString('en-US', { minimumFractionDigits: places, maximumFractionDigits: places })
    : roundTo(x, places).toFixed(places);
  return s + (m[4] ? '%' : '');
}
const FUNCS = {
  SUM: (a) => total(numsOf(a)),
  AVERAGE: (a) => {
    const n = numsOf(a);
    if (!n.length) throw new FormulaError('#DIV/0!');
    return total(n) / n.length;
  },
  COUNT: (a) => {
    let k = 0;
    for (const x of a)
      for (const v of cells(x)) if (typeof v === 'number' || (typeof v === 'string' && looksNumber(v))) k++;
    return k;
  },
  COUNTA: (a) => a.reduce((k, x) => k + cells(x).filter((v) => v !== '' && v != null).length, 0),
  COUNTBLANK: (a) => a.reduce((k, x) => k + cells(x).filter((v) => v === '' || v == null).length, 0),
  MIN: (a) => {
    const n = numsOf(a);
    return n.length ? Math.min(...n) : 0;
  },
  MAX: (a) => {
    const n = numsOf(a);
    return n.length ? Math.max(...n) : 0;
  },
  MEDIAN: (a) => {
    const n = numsOf(a).sort((x, y) => x - y);
    if (!n.length) throw new FormulaError('#NUM!');
    const h = n.length >> 1;
    return n.length % 2 ? n[h] : (n[h - 1] + n[h]) / 2;
  },
  PRODUCT: (a) => numsOf(a).reduce((x, y) => x * y, 1),
  LARGE: (a) => {
    const n = numsOf([a[0]]).sort((x, y) => y - x),
      k = num(a[1]);
    if (k < 1 || k > n.length) throw new FormulaError('#NUM!');
    return n[k - 1];
  },
  SMALL: (a) => {
    const n = numsOf([a[0]]).sort((x, y) => x - y),
      k = num(a[1]);
    if (k < 1 || k > n.length) throw new FormulaError('#NUM!');
    return n[k - 1];
  },
  SUMPRODUCT: (a) => {
    const lists = a.map((x) =>
      cells(x).map((v) => {
        check(v);
        return typeof v === 'number' ? v : typeof v === 'string' && looksNumber(v) ? +v : 0;
      })
    );
    if (lists.some((l) => l.length !== lists[0].length)) throw new FormulaError('#VALUE!');
    return lists[0].reduce((s, _, i) => s + lists.reduce((p, l) => p * l[i], 1), 0);
  },
  ROUND: (a) => roundTo(num(a[0]), num(a[1] ?? 0)),
  ROUNDUP: (a) => roundTo(num(a[0]), num(a[1] ?? 0), 'up'),
  ROUNDDOWN: (a) => roundTo(num(a[0]), num(a[1] ?? 0), 'down'),
  INT: (a) => Math.floor(num(a[0])),
  ABS: (a) => Math.abs(num(a[0])),
  MOD: (a) => {
    const x = num(a[0]),
      y = num(a[1]);
    if (!y) throw new FormulaError('#DIV/0!');
    return x - y * Math.floor(x / y);
  },
  POWER: (a) => num(a[0]) ** num(a[1]),
  SQRT: (a) => {
    const x = num(a[0]);
    if (x < 0) throw new FormulaError('#NUM!');
    return Math.sqrt(x);
  },
  EXP: (a) => Math.exp(num(a[0])),
  LN: (a) => {
    const x = num(a[0]);
    if (x <= 0) throw new FormulaError('#NUM!');
    return Math.log(x);
  },
  LOG10: (a) => {
    const x = num(a[0]);
    if (x <= 0) throw new FormulaError('#NUM!');
    return Math.log10(x);
  },
  PI: () => Math.PI,
  SUMIF: (a) => {
    const hits = ifs([a[0], a[1]]);
    return total(pick(a[2] || a[0], hits));
  },
  SUMIFS: (a) => total(pick(a[0], ifs(a.slice(1)))),
  COUNTIF: (a) => ifs([a[0], a[1]]).length,
  COUNTIFS: (a) => ifs(a).length,
  AVERAGEIF: (a) => {
    const n = pick(a[2] || a[0], ifs([a[0], a[1]]));
    if (!n.length) throw new FormulaError('#DIV/0!');
    return total(n) / n.length;
  },
  AVERAGEIFS: (a) => {
    const n = pick(a[0], ifs(a.slice(1)));
    if (!n.length) throw new FormulaError('#DIV/0!');
    return total(n) / n.length;
  },
  MAXIFS: (a) => {
    const n = pick(a[0], ifs(a.slice(1)));
    return n.length ? Math.max(...n) : 0;
  },
  MINIFS: (a) => {
    const n = pick(a[0], ifs(a.slice(1)));
    return n.length ? Math.min(...n) : 0;
  },
  VLOOKUP: (a) => {
    const t = a[1];
    if (!isRange(t)) throw new FormulaError('#VALUE!');
    const c = num(a[2]);
    if (c < 1 || c > t.rows[0].length) throw new FormulaError('#REF!');
    const exact = a[3] !== undefined && !bool(a[3]);
    return check(
      t.rows[
        lookup(
          a[0],
          t.rows.map((r) => r[0]),
          exact
        )
      ][c - 1]
    );
  },
  HLOOKUP: (a) => {
    const t = a[1];
    if (!isRange(t)) throw new FormulaError('#VALUE!');
    const r = num(a[2]);
    if (r < 1 || r > t.rows.length) throw new FormulaError('#REF!');
    const exact = a[3] !== undefined && !bool(a[3]);
    return check(t.rows[r - 1][lookup(a[0], t.rows[0], exact)]);
  },
  INDEX: (a) => {
    const t = isRange(a[0]) ? a[0] : { rows: [[a[0]]] };
    let r = num(a[1] ?? 0),
      c = a[2] === undefined ? 0 : num(a[2]);
    if (t.rows.length === 1 && a[2] === undefined) {
      c = r;
      r = 1;
    }
    if (!c) c = 1;
    if (!r) r = 1;
    const row = t.rows[r - 1];
    if (!row || c > row.length) throw new FormulaError('#REF!');
    return check(row[c - 1]);
  },
  MATCH: (a) => {
    const list = cells(a[1]),
      how = a[2] === undefined ? 1 : num(a[2]);
    if (how === 0) return lookup(a[0], list, true) + 1;
    if (how > 0) return lookup(a[0], list, false) + 1;
    const key = scalar(a[0]);
    let at = -1;
    for (let i = 0; i < list.length; i++)
      if (order(list[i], key) >= 0) at = i;
      else break;
    if (at < 0) throw new FormulaError('#N/A');
    return at + 1;
  },
  XLOOKUP: (a) => {
    const keys = cells(a[1]),
      out = cells(a[2]);
    let i;
    try {
      i = lookup(a[0], keys, true);
    } catch (e) {
      if (a[3] !== undefined) return a[3];
      throw e;
    }
    return check(out[i]);
  },
  CHOOSE: (a) => {
    const k = Math.floor(num(a[0]));
    if (k < 1 || k >= a.length) throw new FormulaError('#VALUE!');
    return a[k];
  },
  AND: (a) => {
    const v = a.flatMap(cells).filter((x) => x !== '');
    if (!v.length) throw new FormulaError('#VALUE!');
    return v.every((x) => bool(x));
  },
  OR: (a) => {
    const v = a.flatMap(cells).filter((x) => x !== '');
    if (!v.length) throw new FormulaError('#VALUE!');
    return v.some((x) => bool(x));
  },
  NOT: (a) => !bool(a[0]),
  CONCAT: (a) => a.flatMap(cells).map(str).join(''),
  CONCATENATE: (a) => a.map(str).join(''),
  TEXTJOIN: (a) => {
    const sep = str(a[0]),
      skip = bool(a[1]);
    return a
      .slice(2)
      .flatMap(cells)
      .map(str)
      .filter((s) => !skip || s !== '')
      .join(sep);
  },
  LEN: (a) => [...str(a[0])].length,
  LEFT: (a) => [...str(a[0])].slice(0, a[1] === undefined ? 1 : num(a[1])).join(''),
  RIGHT: (a) => {
    const s = [...str(a[0])],
      k = a[1] === undefined ? 1 : num(a[1]);
    return k ? s.slice(-k).join('') : '';
  },
  MID: (a) => {
    const s = [...str(a[0])],
      at = num(a[1]),
      k = num(a[2]);
    if (at < 1 || k < 0) throw new FormulaError('#VALUE!');
    return s.slice(at - 1, at - 1 + k).join('');
  },
  UPPER: (a) => str(a[0]).toUpperCase(),
  LOWER: (a) => str(a[0]).toLowerCase(),
  PROPER: (a) =>
    str(a[0])
      .toLowerCase()
      .replace(/(^|[^a-z])([a-z])/g, (m, b, c) => b + c.toUpperCase()),
  TRIM: (a) => str(a[0]).replace(/ +/g, ' ').trim(),
  REPT: (a) => str(a[0]).repeat(Math.max(0, num(a[1]))),
  EXACT: (a) => str(a[0]) === str(a[1]),
  SUBSTITUTE: (a) => {
    const s = str(a[0]),
      from = str(a[1]),
      to = str(a[2]);
    if (!from) return s;
    if (a[3] === undefined) return s.split(from).join(to);
    let k = num(a[3]),
      at = -1;
    while (k-- > 0) {
      at = s.indexOf(from, at + 1);
      if (at < 0) return s;
    }
    return s.slice(0, at) + to + s.slice(at + from.length);
  },
  FIND: (a) => {
    const at = str(a[1]).indexOf(str(a[0]), (a[2] === undefined ? 1 : num(a[2])) - 1);
    if (at < 0) throw new FormulaError('#VALUE!');
    return at + 1;
  },
  SEARCH: (a) => {
    const at = str(a[1])
      .toLowerCase()
      .indexOf(str(a[0]).toLowerCase(), (a[2] === undefined ? 1 : num(a[2])) - 1);
    if (at < 0) throw new FormulaError('#VALUE!');
    return at + 1;
  },
  VALUE: (a) => num(a[0]),
  TEXT: (a) => text(a[0], a[1]),
  DATE: (a) => {
    const y = num(a[0]),
      m = num(a[1]),
      d = num(a[2]);
    return Date.UTC(y < 1900 ? y + 1900 : y, m - 1, d) / 86400000 + epoch.base;
  },
  YEAR: (a) => day(a[0]).y,
  MONTH: (a) => day(a[0]).m,
  DAY: (a) => day(a[0]).d,
  WEEKDAY: (a) => {
    const w = day(a[0]).wd,
      t = a[1] === undefined ? 1 : num(a[1]);
    return t === 2 ? ((w + 6) % 7) + 1 : t === 3 ? (w + 6) % 7 : w + 1;
  },
  TODAY: () => {
    const n = new Date();
    return Date.UTC(n.getFullYear(), n.getMonth(), n.getDate()) / 86400000 + epoch.base;
  },
  NOW: () => {
    const n = new Date();
    return (n.getTime() - n.getTimezoneOffset() * 60000) / 86400000 + epoch.base;
  },
  ISBLANK: (a) => {
    const v = scalar(a[0]);
    return v === '' || v == null;
  },
  ISNUMBER: (a) => typeof scalar(a[0]) === 'number',
  ISTEXT: (a) => {
    const v = scalar(a[0]);
    return typeof v === 'string' && v !== '';
  },
  ISERROR: (a) => scalar(a[0]) instanceof FormulaError,
  NA: () => {
    throw new FormulaError('#N/A');
  },
};
// these take what they're given unworked, so an error or the side not taken never reaches them
const LAZY = {
  IF: (args, run) => {
    const t = bool(run(args[0]));
    const k = t ? args[1] : args[2];
    return k === undefined ? t : k.t === 'blank' ? 0 : run(k);
  },
  IFERROR: (args, run) => {
    try {
      const v = run(args[0]);
      check(scalar(v));
      return v;
    } catch (e) {
      if (e instanceof FormulaError) return run(args[1]);
      throw e;
    }
  },
  IFNA: (args, run) => {
    try {
      const v = run(args[0]);
      check(scalar(v));
      return v;
    } catch (e) {
      if (e instanceof FormulaError && e.code === '#N/A') return run(args[1]);
      throw e;
    }
  },
  IFS: (args, run) => {
    for (let i = 0; i + 1 < args.length; i += 2) if (bool(run(args[i]))) return run(args[i + 1]);
    throw new FormulaError('#N/A');
  },
  ISERROR: (args, run) => {
    try {
      check(scalar(run(args[0])));
      return false;
    } catch (e) {
      if (e instanceof FormulaError) return true;
      throw e;
    }
  },
  ISNUMBER: (args, run) => {
    try {
      return typeof check(scalar(run(args[0]))) === 'number';
    } catch (e) {
      if (e instanceof FormulaError) return false;
      throw e;
    }
  },
};
// functions newer than the file format, which files keep with a prefix (CONCAT is _xlfn.CONCAT)
export const NEWER = new Set(['CONCAT', 'TEXTJOIN', 'MAXIFS', 'MINIFS', 'IFS', 'XLOOKUP', 'IFNA', 'SWITCH']);
const pos = (ref) => {
  const m = /^([A-Z]+)(\d+)$/.exec(ref);
  return { c: colNo(m[1]), r: +m[2] - 1 };
};

/**
 * A formula's value. `ctx.read(sheet, r, c)` gives a cell's value (or throws its error); `ctx.name(n)` what a defined
 * name stands for (a reference like "Sales!$F$2:$F$13"), or null.
 */
export function evaluate(source, ctx) {
  const tree = typeof source === 'string' ? parse(source) : source;
  const range = (sheet, a, b) => {
    const p = pos(a),
      q = pos(b),
      rows = [];
    for (let r = Math.min(p.r, q.r); r <= Math.max(p.r, q.r); r++) {
      const row = [];
      for (let c = Math.min(p.c, q.c); c <= Math.max(p.c, q.c); c++) {
        try {
          row.push(ctx.read(sheet, r, c));
        } catch (e) {
          if (!(e instanceof FormulaError)) throw e;
          row.push(e);
        }
      }
      rows.push(row);
    }
    return { rows };
  };
  const run = (n) => {
    switch (n.t) {
      case 'num':
      case 'str':
      case 'bool':
        return n.v;
      case 'blank':
        return '';
      case 'err':
        throw new FormulaError(n.v);
      case 'ref': {
        if (!n.b) {
          const p = pos(n.a);
          return ctx.read(n.sheet, p.r, p.c);
        }
        return range(n.sheet, n.a, n.b);
      }
      case 'name': {
        const to = ctx.name && ctx.name(n.v);
        if (!to) throw new NotSupported();
        return run(parse(to));
      }
      case 'neg':
        return n.op === '-' ? -num(run(n.a)) : num(run(n.a));
      case 'pct':
        return num(run(n.a)) / 100;
      case 'bin': {
        const a = run(n.a),
          b = run(n.b);
        switch (n.op) {
          case '+':
            return num(a) + num(b);
          case '-':
            return num(a) - num(b);
          case '*':
            return num(a) * num(b);
          case '/': {
            const d = num(b),
              x = num(a);
            if (!d) throw new FormulaError('#DIV/0!');
            return x / d;
          }
          case '^':
            return num(a) ** num(b);
          case '&':
            return str(a) + str(b);
          default: {
            let x = check(scalar(a)),
              y = check(scalar(b));
            if (typeof x === 'string' && looksNumber(x)) x = +x;
            if (typeof y === 'string' && looksNumber(y)) y = +y;
            if (n.op === '=') return same(x, y);
            if (n.op === '<>') return !same(x, y);
            const d = order(x === '' && typeof y === 'string' ? '' : x, y === '' && typeof x === 'string' ? '' : y);
            return n.op === '<' ? d < 0 : n.op === '>' ? d > 0 : n.op === '<=' ? d <= 0 : d >= 0;
          }
        }
      }
      case 'fn': {
        if (LAZY[n.v]) return LAZY[n.v](n.args, run);
        const f = FUNCS[n.v];
        if (!f) throw new NotSupported();
        return f(n.args.map((x) => (x.t === 'blank' ? undefined : run(x))));
      }
    }
    throw new NotSupported();
  };
  const v = run(tree);
  return check(scalar(v));
}

/** A formula as the person wrote it, with names and references in capitals as every spreadsheet keeps them. */
export function tidyFormula(f) {
  return f.replace(/("(?:[^"]|"")*")|('(?:[^']|'')+'!)|([^"']+)/g, (all, s, sh, rest) =>
    rest == null
      ? all
      : rest
          .replace(/(?<![A-Za-z0-9_.])([A-Za-z_][A-Za-z0-9_.]*)(?=\()/g, (m) => m.toUpperCase())
          .replace(
            /(?<![A-Za-z0-9_.])(\$?)([A-Za-z]{1,3})(\$?)(\d+)(?![A-Za-z0-9_(.])/g,
            (m, a, c, b, r) => a + c.toUpperCase() + b + r
          )
          .replace(/(?<![A-Za-z0-9_.])(true|false)(?![A-Za-z0-9_(.])/gi, (m) => m.toUpperCase())
  );
}

// ---------- references that move when rows or columns come and go ----------
const REF =
  /((?:'(?:[^']|'')+'|[^\s!'"(),+\-*/&=<>:;]+)!)?(\$?)([A-Z]{1,3})(\$?)(\d+)(?::(\$?)([A-Z]{1,3})(\$?)(\d+))?(?![A-Za-z0-9_(])/g;
const sheetOf = (sh) => sh.slice(0, -1).replace(/^'|'$/g, '').replace(/''/g, "'");
/**
 * Every reference to sheet `name` in `f`, moved for `by` rows (or columns) added at `at` (taken away when `by` < 0).
 * `here` is the sheet a bare reference means (null: bare references are left alone). Text in quotes is never touched.
 * A reference to what was taken away becomes #REF!.
 */
export function moveRefs(f, name, here, isRow, at, by) {
  const shift = (v) => {
    if (by > 0) return v >= at ? v + by : v;
    if (v >= at && v < at - by) return null;
    return v >= at - by ? v + by : v;
  };
  const one = (seg) =>
    seg.replace(REF, (all, sh, d1, c1, d2, r1, d3, c2, d4, r2) => {
      const target = sh ? sheetOf(sh) : here;
      if (target == null || target.toLowerCase() !== String(name).toLowerCase()) return all;
      const write = (x, dc, dr) => dc + colName(x.c) + dr + (x.r + 1);
      const a = isRow ? { c: colNo(c1), r: shift(+r1 - 1) } : { c: shift(colNo(c1)), r: +r1 - 1 };
      if (c2 == null) return a.r == null || a.c == null ? '#REF!' : (sh || '') + write(a, d1, d2);
      let b = isRow ? { c: colNo(c2), r: +r2 - 1 } : { c: colNo(c2), r: +r2 - 1 };
      // a range keeps what's left of it: its far end moves with the rows or columns before it
      const lo = isRow ? +r1 - 1 : colNo(c1),
        hi = isRow ? +r2 - 1 : colNo(c2);
      let nlo, nhi;
      if (by > 0) {
        nlo = lo >= at ? lo + by : lo;
        nhi = hi >= at ? hi + by : hi;
      } else {
        const gone = -by,
          end = at + gone - 1;
        nlo = lo < at ? lo : lo > end ? lo - gone : at;
        nhi = hi < at ? hi : hi > end ? hi - gone : at - 1;
        if (nhi < nlo) return '#REF!';
      }
      const start = isRow ? { c: a.c, r: nlo } : { c: nlo, r: a.r };
      b = isRow ? { c: b.c, r: nhi } : { c: nhi, r: b.r };
      return (sh || '') + write(start, d1, d2) + ':' + write(b, d3, d4);
    });
  return f.replace(/("(?:[^"]|"")*")|([^"]+)/g, (all, s, rest) => (rest == null ? all : one(rest)));
}
/** A formula copied from one cell to another: its relative references move by the same rows and columns. */
export function shiftFormula(f, dr, dc) {
  const one = (seg) =>
    seg.replace(REF, (all, sh, d1, c1, d2, r1, d3, c2, d4, r2) => {
      const w = (d, c, e, r) => {
        const cn = d ? colNo(c) : colNo(c) + dc,
          rn = e ? +r : +r + dr;
        return cn < 0 || rn < 1 ? null : d + colName(cn) + e + rn;
      };
      const a = w(d1, c1, d2, r1);
      if (a == null) return '#REF!';
      if (c2 == null) return (sh || '') + a;
      const b = w(d3, c2, d4, r2);
      return b == null ? '#REF!' : (sh || '') + a + ':' + b;
    });
  return f.replace(/("(?:[^"]|"")*")|([^"]+)/g, (all, s, rest) => (rest == null ? all : one(rest)));
}
