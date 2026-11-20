// XLSX and XLSM, read and written by the page. Reading keeps every cell's own XML beside its value; writing puts that XML
// back for every cell nobody changed, rewrites only the sheets that changed, and leaves every other part of the package
// to be copied as it was (shared/table/src/package.rs). Charts, pivot tables, pictures, comments and styles are never
// rewritten; when rows or columns come and go, only the references inside them move.
import { unzipSync, strFromU8 } from 'fflate';
import { colNo, colName, moveRefs, shiftFormula, epoch, NEWER, FormulaError, NotSupported } from './sheet.js';

// ---------- XML as text ----------
const unesc = (s) =>
  s.indexOf('&') < 0 && s.indexOf('_x') < 0
    ? s
    : s
        .replace(/&(lt|gt|amp|quot|apos|#\d+|#x[0-9a-fA-F]+);/g, (m, e) =>
          e === 'lt'
            ? '<'
            : e === 'gt'
              ? '>'
              : e === 'amp'
                ? '&'
                : e === 'quot'
                  ? '"'
                  : e === 'apos'
                    ? "'"
                    : String.fromCodePoint(e[1] === 'x' ? parseInt(e.slice(2), 16) : +e.slice(1))
        )
        .replace(/_x([0-9A-Fa-f]{4})_/g, (m, h) => String.fromCharCode(parseInt(h, 16)));
const esc = (s) =>
  s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/_(x[0-9A-Fa-f]{4}_)/g, '_x005F_$1')
    .replace(
      /[\u0000-\u0008\u000B\u000C\u000E-\u001F\uFFFE\uFFFF]/g,
      (c) => '_x' + c.charCodeAt(0).toString(16).padStart(4, '0').toUpperCase() + '_'
    );
const escAttr = (s) => esc(s).replace(/"/g, '&quot;');
const attr = (tag, name) => {
  const m = new RegExp('\\s' + name + '="([^"]*)"').exec(tag);
  return m ? unesc(m[1]) : null;
};
const texts = (xml) => {
  let out = '';
  const re = /<(?:\w+:)?t(?:\s[^>]*)?(?:\/>|>([\s\S]*?)<\/(?:\w+:)?t>)/g;
  let m;
  while ((m = re.exec(xml))) out += unesc(m[1] || '');
  return out;
};
const join = (base, target) => {
  if (target.startsWith('/')) return target.slice(1);
  const parts = base.split('/');
  parts.pop();
  for (const p of target.split('/')) {
    if (p === '..') parts.pop();
    else if (p !== '.') parts.push(p);
  }
  return parts.join('/');
};
const relsOf = (files, part) => {
  const at = part.lastIndexOf('/'),
    p = part.slice(0, at) + '/_rels/' + part.slice(at + 1) + '.rels';
  return files[p] ? strFromU8(files[p]) : '';
};
const relList = (xml) => {
  const out = [];
  const re = /<Relationship\b[^>]*>/g;
  let m;
  while ((m = re.exec(xml)))
    out.push({
      id: attr(m[0], 'Id'),
      type: attr(m[0], 'Type') || '',
      target: attr(m[0], 'Target') || '',
      mode: attr(m[0], 'TargetMode'),
    });
  return out;
};

function fmtOfNumFmt(id, code) {
  if (id === 9 || id === 10) return '%';
  if ((id >= 14 && id <= 17) || id === 22) return 'date';
  if (id === 3 || id === 4) return ',';
  if (code) {
    const c = code.replace(/"[^"]*"|\[[^\]]*\]|\\./g, '');
    if (/%/.test(c)) return '%';
    if (/y/i.test(c) || (/d/i.test(c) && /m/i.test(c))) return 'date';
    if (/#,##/.test(c)) return ',';
  }
  return '';
}

// ---------- reading ----------
export function readXlsx(bytes) {
  const files = unzipSync(bytes),
    text = (p) => (files[p] ? strFromU8(files[p]) : null);
  const wbPart = Object.keys(files).find((p) => p.toLowerCase() === 'xl/workbook.xml');
  const wbXml = text(wbPart);
  if (!wbXml) throw new Error('no workbook');
  const rels = relList(relsOf(files, wbPart));
  if (/<(?:\w+:)?workbookPr\b[^>]*\bdate1904="(1|true)"/.test(wbXml)) epoch.base = 24107;
  else epoch.base = 25569;
  // shared strings: the words of a cell, without the small reading guides some languages add above them
  const shared = [];
  const sstRel = rels.find((r) => /\/sharedStrings$/.test(r.type));
  const sst = sstRel ? text(join(wbPart, sstRel.target)) : null;
  if (sst) {
    const re = /<(?:\w+:)?si>([\s\S]*?)<\/(?:\w+:)?si>|<(?:\w+:)?si\/>/g;
    let m;
    while ((m = re.exec(sst)))
      shared.push(m[1] ? texts(m[1].replace(/<(?:\w+:)?rPh\b[\s\S]*?<\/(?:\w+:)?rPh>/g, '')) : '');
  }
  const fmts = [];
  const styRel = rels.find((r) => /\/styles$/.test(r.type));
  const sty = styRel ? text(join(wbPart, styRel.target)) : null;
  if (sty) {
    const custom = {};
    let m;
    const nf = /<(?:\w+:)?numFmt\b[^>]*>/g;
    while ((m = nf.exec(sty))) custom[+attr(m[0], 'numFmtId')] = attr(m[0], 'formatCode');
    const xfs = /<(?:\w+:)?cellXfs\b[^>]*>([\s\S]*?)<\/(?:\w+:)?cellXfs>/.exec(sty);
    if (xfs) {
      const re = /<(?:\w+:)?xf\b[^>]*>/g;
      while ((m = re.exec(xfs[1]))) {
        const id = +(attr(m[0], 'numFmtId') || 0);
        fmts.push(fmtOfNumFmt(id, custom[id]));
      }
    }
  }
  // names a formula may use: Revenue for Sales!$F$2:$F$13
  const names = {};
  {
    const re = /<(?:\w+:)?definedName\b([^>]*)>([\s\S]*?)<\/(?:\w+:)?definedName>/g;
    let m;
    while ((m = re.exec(wbXml))) if (!/localSheetId=/.test(m[1])) names[attr(m[0], 'name').toUpperCase()] = unesc(m[2]);
  }
  const sheets = [];
  const re = /<(?:\w+:)?sheet\b[^>]*>/g;
  let m;
  while ((m = re.exec(wbXml))) {
    const rid = attr(m[0], 'r:id') || (/\s\w+:id="([^"]*)"/.exec(m[0]) || [])[1],
      rel = rels.find((r) => r.id === rid);
    if (!rel || !/\/worksheet$/.test(rel.type)) continue;
    const part = join(wbPart, rel.target),
      xml = text(part);
    sheets.push(readSheet(attr(m[0], 'name'), part, xml || '<worksheet><sheetData/></worksheet>', shared, fmts));
  }
  if (!sheets.length) throw new Error('no sheets');
  const paths = Object.keys(files);
  const extras = {
    charts: paths.filter((p) => /^xl\/charts\/chart\d+\.xml$/i.test(p)).length,
    pivots: paths.filter((p) => /^xl\/pivotTables\/[^/]*\.xml$/i.test(p)).length,
    macros: paths.some((p) => /vbaProject\.bin$/i.test(p)),
  };
  return { sheets, extras, names, pkg: { files, wbPart, wbXml } };
}

function readSheet(name, part, xml, shared, fmts) {
  const rows = [],
    src = [],
    sty = [],
    rowAttr = [],
    widths = {};
  const cm = /<(?:\w+:)?cols>([\s\S]*?)<\/(?:\w+:)?cols>/.exec(xml);
  if (cm) {
    const re = /<(?:\w+:)?col\b[^>]*>/g;
    let m;
    while ((m = re.exec(cm[1]))) {
      const a = +attr(m[0], 'min') - 1,
        b = +attr(m[0], 'max') - 1,
        w = +attr(m[0], 'width');
      if (w && b - a < 200) for (let i = a; i <= b; i++) widths[i] = Math.round(w * 7.2);
    }
  }
  const open = /<(?:\w+:)?sheetData\b[^>]*?(\/?)>/.exec(xml);
  let dataAt = [xml.length, xml.length];
  if (open) {
    const start = open.index,
      after = start + open[0].length;
    const shut = open[1] ? null : /<\/(?:\w+:)?sheetData>/g;
    if (shut) shut.lastIndex = after;
    const m = shut && shut.exec(xml);
    dataAt = [start, m ? m.index + m[0].length : after];
    const body = m ? xml.slice(after, m.index) : '';
    const masters = {};
    const rowRe = /<(?:\w+:)?row\b([^>]*?)(?:\/>|>([\s\S]*?)<\/(?:\w+:)?row>)/g;
    let rm,
      nextRow = 0;
    while ((rm = rowRe.exec(body))) {
      const ra = rm[1],
        rAt = attr(' ' + ra, 'r'),
        ri = rAt ? +rAt - 1 : nextRow;
      nextRow = ri + 1;
      const out = (rows[ri] = []),
        so = (src[ri] = []),
        st = (sty[ri] = []);
      const rest = ra.replace(/\s(r|spans)="[^"]*"/g, '').trim();
      if (rest) rowAttr[ri] = rest;
      if (!rm[2]) continue;
      const cRe = /<(?:\w+:)?c\b([^>]*?)(?:\/>|>([\s\S]*?)<\/(?:\w+:)?c>)/g;
      let c,
        nextCol = 0;
      while ((c = cRe.exec(rm[2]))) {
        const ca = ' ' + c[1],
          ref = attr(ca, 'r'),
          ci = ref ? colNo(ref) : nextCol;
        nextCol = ci + 1;
        const type = attr(ca, 't'),
          s = attr(ca, 's'),
          fmt = fmts[+(s || 0)] || '',
          inner = c[2] || '';
        so[ci] = c[0];
        if (s != null) st[ci] = s;
        const v = /<(?:\w+:)?v>([\s\S]*?)<\/(?:\w+:)?v>/.exec(inner),
          raw = v ? unesc(v[1]) : '';
        let val;
        if (type === 's') val = shared[+raw] ?? '';
        else if (type === 'inlineStr') {
          const is = /<(?:\w+:)?is>([\s\S]*?)<\/(?:\w+:)?is>/.exec(inner);
          val = is ? texts(is[1]) : '';
        } else if (type === 'str' || type === 'e') val = raw;
        else if (type === 'b') val = raw === '1' ? 'TRUE' : 'FALSE';
        else val = raw === '' ? '' : { n: +raw, fmt };
        const f = /<(?:\w+:)?f\b([^>]*?)(?:\/>|>([\s\S]*?)<\/(?:\w+:)?f>)/.exec(inner);
        let formula = f && f[2] ? unesc(f[2]) : null;
        if (f && attr(' ' + f[1], 't') === 'shared') {
          const si = attr(' ' + f[1], 'si');
          if (formula) masters[si] = { f: formula, r: ri, c: ci };
          else if (masters[si])
            formula = shiftFormula('=' + masters[si].f, ri - masters[si].r, ci - masters[si].c).slice(1);
        }
        out[ci] = formula ? { f: '=' + formula.replace(/_xlfn\.|_xlws\./g, ''), c: val, fmt } : val;
      }
    }
  }
  for (let i = 0; i < rows.length; i++)
    if (!rows[i]) {
      rows[i] = [];
      src[i] = [];
      sty[i] = [];
    }
  return { name, rows, widths, x: { part, xml, dataAt, src, sty, rowAttr, moves: [], touched: false } };
}

// ---------- writing ----------
const addPrefix = (f) =>
  f.replace(/("(?:[^"]|"")*")|([^"]+)/g, (all, s, rest) =>
    rest == null
      ? all
      : rest.replace(/(?<![A-Za-z0-9_.])([A-Z][A-Z0-9.]*)(?=\()/g, (n) => (NEWER.has(n) ? '_xlfn.' + n : n))
  );
/** One cell's XML. `value` gives a formula's result (or throws); `fmtIndex` is the style for a fresh package. */
function cellXml(ref, x, s, value) {
  const sa = s != null ? ` s="${s}"` : '';
  if (x == null || x === '') return s != null ? `<c r="${ref}"${sa}/>` : '';
  if (typeof x === 'string') {
    // a number someone typed is a number; "007" and a long ID stay words
    if (/^-?(0|[1-9]\d{0,14})(\.\d+)?$/.test(x)) return `<c r="${ref}"${sa}><v>${x}</v></c>`;
    const sp = /^\s|\s$|\n/.test(x) ? ' xml:space="preserve"' : '';
    return `<c r="${ref}"${sa} t="inlineStr"><is><t${sp}>${esc(x)}</t></is></c>`;
  }
  if (x.f == null) return `<c r="${ref}"${sa}><v>${x.n}</v></c>`;
  const f = `<f>${esc(addPrefix(x.f.replace(/^=/, '')))}</f>`;
  let v;
  try {
    v = value();
  } catch (e) {
    if (e instanceof FormulaError) return `<c r="${ref}"${sa} t="e">${f}<v>${e.code}</v></c>`;
    // a formula this code can't work out keeps the result it came with, and the spreadsheet that opens it works it out again
    v = x.c == null || x.c === '' ? null : typeof x.c === 'object' ? x.c.n : x.c;
    if (v == null) return `<c r="${ref}"${sa}>${f}</c>`;
    if (typeof v === 'string' && /^#[A-Z/0!?]+[!?A]$/.test(v))
      return `<c r="${ref}"${sa} t="e">${f}<v>${esc(v)}</v></c>`;
  }
  if (typeof v === 'number') return `<c r="${ref}"${sa}>${f}<v>${v}</v></c>`;
  if (typeof v === 'boolean') return `<c r="${ref}"${sa} t="b">${f}<v>${v ? 1 : 0}</v></c>`;
  return `<c r="${ref}"${sa} t="str">${f}<v>${esc(String(v))}</v></c>`;
}

function sheetData(s, value, fresh) {
  const out = ['<sheetData>'],
    x = s.x || {};
  let maxR = 0,
    maxC = 0;
  for (let r = 0; r < s.rows.length; r++) {
    const row = s.rows[r] || [],
      so = (x.src && x.src[r]) || [],
      st = (x.sty && x.sty[r]) || [],
      ra = x.rowAttr && x.rowAttr[r];
    const n = Math.max(row.length, so.length, st.length),
      cellsOut = [];
    for (let c = 0; c < n; c++) {
      const ref = colName(c) + (r + 1);
      let xml;
      // a cell nobody changed is its own XML again, at its new address; a shared formula is written out in full
      if (so[c] && !/\st="shared"/.test(so[c])) xml = so[c].replace(/^(<(?:\w+:)?c\b[^>]*?\sr=")[^"]*"/, `$1${ref}"`);
      else xml = cellXml(ref, row[c], fresh ? fmtStyle(row[c]) : st[c], () => value(s, r, c));
      if (xml) {
        cellsOut.push(xml);
        maxC = Math.max(maxC, c);
      }
    }
    if (!cellsOut.length && !ra) continue;
    maxR = r;
    out.push(`<row r="${r + 1}"${ra ? ' ' + ra : ''}${cellsOut.length ? '>' + cellsOut.join('') + '</row>' : '/>'}`);
  }
  out.push('</sheetData>');
  return { xml: out.join(''), dim: 'A1' + (maxR || maxC ? ':' + colName(maxC) + (maxR + 1) : '') };
}
const fmtStyle = (x) => {
  const f = x && typeof x === 'object' ? x.fmt : '';
  return f === 'date' ? '1' : f === '%' ? '2' : f === ',' ? '3' : undefined;
};

// the references in a sheet's own XML outside its cells: merged cells, conditional formats, validation, links, filters, views
function moveSheetOutside(text, name, mv) {
  const one = (ref) => moveRefs(ref, name, name, mv.isRow, mv.at, mv.by);
  const list = (v) =>
    v
      .split(/\s+/)
      .filter(Boolean)
      .map(one)
      .filter((r) => !/#REF!/.test(r))
      .join(' ');
  let t = text;
  // whole elements whose one range is gone go with it
  t = t.replace(/<(?:\w+:)?(mergeCell|hyperlink)\b[^>]*\sref="([^"]*)"[^>]*\/>/g, (all, tag, ref) => {
    const n = one(ref);
    return /#REF!/.test(n) ? '' : all.replace(`ref="${ref}"`, `ref="${n}"`);
  });
  t = t.replace(
    /<((?:\w+:)?(conditionalFormatting|dataValidation))\b([^>]*)\ssqref="([^"]*)"([\s\S]*?)<\/\1>/g,
    (all, full, tag, a, ref, rest) => {
      const n = list(ref);
      return n ? all.replace(`sqref="${ref}"`, `sqref="${n}"`) : '';
    }
  );
  t = t.replace(/\s(ref|sqref|activeCell|topLeftCell)="([^"]*)"/g, (all, k, v) => {
    const n = k === 'sqref' ? list(v) : one(v);
    return /#REF!/.test(n) || !n ? all : ` ${k}="${n}"`;
  });
  t = t.replace(
    /<((?:\w+:)?(?:formula|f|sqref))>([^<]*)<\/\1>/g,
    (all, tag, v) => `<${tag}>${esc(moveRefs(unesc(v), name, name, mv.isRow, mv.at, mv.by))}</${tag}>`
  );
  t = t.replace(/(<\w+:sqref>)([^<]*)(<\/\w+:sqref>)/g, (all, a, v, b) => a + list(v) + b);
  if (!mv.isRow)
    t = t.replace(/<((?:\w+:)?col)\b([^>]*)\/>/g, (all, tag, a) => {
      const lo = +attr(a, 'min') - 1,
        hi = +attr(a, 'max') - 1,
        gone = -mv.by;
      let nlo, nhi;
      if (mv.by > 0) {
        nlo = lo >= mv.at ? lo + mv.by : lo;
        nhi = hi >= mv.at ? hi + mv.by : hi;
      } else {
        const end = mv.at + gone - 1;
        nlo = lo < mv.at ? lo : lo > end ? lo - gone : mv.at;
        nhi = hi < mv.at ? hi : hi > end ? hi - gone : mv.at - 1;
        if (nhi < nlo) return '';
      }
      return all.replace(/\smin="\d+"/, ` min="${nlo + 1}"`).replace(/\smax="\d+"/, ` max="${nhi + 1}"`);
    });
  return t;
}

const CT_XLSX = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml';
/**
 * What saving as XLSX writes over the file the book was opened from. `book` is the sheets (each with its `x` from
 * readXlsx), `pkg` the package as read, `value(sheet, r, c)` a formula's result. Returns { parts, remove }.
 */
export function patchXlsx(book, pkg, value) {
  const { files } = pkg,
    parts = {},
    remove = [],
    text = (p) => (parts[p] != null ? parts[p] : files[p] ? strFromU8(files[p]) : null);
  const set = (p, t) => {
    if (t !== text(p)) parts[p] = t;
  };
  const touched = book.filter((s) => s.x && s.x.touched);
  // the moves, every sheet's, in the order they were made
  const moves = book
    .flatMap((s) => (s.x ? s.x.moves.map((m) => ({ ...m, name: s.name })) : []))
    .sort((a, b) => a.seq - b.seq);
  for (const s of touched) {
    const x = s.x,
      { xml: data, dim } = sheetData(s, value, false);
    let head = x.xml.slice(0, x.dataAt[0]),
      tail = x.xml.slice(x.dataAt[1]);
    for (const mv of x.moves) {
      head = moveSheetOutside(head, s.name, mv);
      tail = moveSheetOutside(tail, s.name, mv);
    }
    head = head.replace(/(<(?:\w+:)?dimension\b[^>]*\sref=")[^"]*"/, `$1${dim}"`);
    set(x.part, head + data + tail);
  }
  if (moves.length) {
    const moveAll = (f, here) => moves.reduce((t, mv) => moveRefs(t, mv.name, here, mv.isRow, mv.at, mv.by), f);
    // names in the workbook, ranges in charts, the source of each pivot table, the extent of each table
    set(
      pkg.wbPart,
      text(pkg.wbPart).replace(
        /(<(?:\w+:)?definedName\b[^>]*>)([\s\S]*?)(<\/(?:\w+:)?definedName>)/g,
        (all, a, v, b) => a + esc(moveAll(unesc(v), null)) + b
      )
    );
    for (const p of Object.keys(files)) {
      if (/^xl\/charts\/chart[^/]*\.xml$/i.test(p) || /^xl\/chartsheets\//i.test(p))
        set(
          p,
          text(p).replace(
            /(<(?:\w+:)?f>)([\s\S]*?)(<\/(?:\w+:)?f>)/g,
            (all, a, v, b) => a + esc(moveAll(unesc(v), null)) + b
          )
        );
      else if (/^xl\/pivotCache\/pivotCacheDefinition[^/]*\.xml$/i.test(p))
        set(
          p,
          text(p).replace(/<(?:\w+:)?worksheetSource\b[^>]*>/g, (tag) => {
            const ref = attr(tag, 'ref'),
              sh = attr(tag, 'sheet');
            if (!ref || !sh) return tag;
            const n = moveAll(ref, sh);
            return /#REF!/.test(n) ? tag : tag.replace(`ref="${ref}"`, `ref="${n}"`);
          })
        );
    }
    for (const s of book) {
      if (!s.x) continue;
      for (const r of relList(relsOf(files, s.x.part))) {
        if (!/\/table$/.test(r.type)) continue;
        const p = join(s.x.part, r.target),
          t = text(p);
        if (!t) continue;
        set(
          p,
          t.replace(/(<(?:\w+:)?(?:table|autoFilter)\b[^>]*\sref=")([^"]*)"/g, (all, a, ref) => {
            const n = moves.reduce((v, mv) => moveRefs(v, mv.name, s.name, mv.isRow, mv.at, mv.by), ref);
            return /#REF!/.test(n) ? all : a + n + '"';
          })
        );
      }
    }
  }
  const wbRelsPart = pkg.wbPart.replace(/[^/]*$/, '_rels/') + pkg.wbPart.split('/').pop() + '.rels';
  // the cached order of calculation describes cells that may have moved or changed: Excel rebuilds it, and works out
  // every formula again when the file opens
  if (touched.length) {
    const chain = Object.keys(files).find((p) => /^xl\/calcChain\.xml$/i.test(p));
    if (chain) {
      remove.push(chain);
      set(wbRelsPart, text(wbRelsPart).replace(/<Relationship\b[^>]*\/calcChain"[^>]*\/>/g, ''));
      set(
        '[Content_Types].xml',
        text('[Content_Types].xml').replace(/<Override\b[^>]*PartName="\/xl\/calcChain\.xml"[^>]*\/>/gi, '')
      );
    }
    const wb = text(pkg.wbPart);
    set(
      pkg.wbPart,
      /<(?:\w+:)?calcPr\b/.test(wb)
        ? wb.replace(
            /<((?:\w+:)?calcPr)\b([^>]*?)(\/?)>/,
            (all, tag, a, sl) => `<${tag}${a.replace(/\sfullCalcOnLoad="[^"]*"/, '')} fullCalcOnLoad="1"${sl}>`
          )
        : wb.replace(
            /(<\/(?:\w+:)?sheets>(?:\s*<(?:\w+:)?(?:functionGroups|externalReferences|definedNames)\b[\s\S]*?<\/(?:\w+:)?(?:functionGroups|externalReferences|definedNames)>)*)/,
            '$1<calcPr fullCalcOnLoad="1"/>'
          )
    );
  }
  // XLSM saved as XLSX: the macros stay behind
  if (
    Object.keys(files).some((p) => /vbaProject\.bin$/i.test(p))
    || /macroEnabled/i.test(text('[Content_Types].xml'))
  ) {
    for (const p of Object.keys(files))
      if (/^xl\/vba[^/]*\.(bin|xml)$/i.test(p) || /^xl\/_rels\/vba[^/]*\.rels$/i.test(p)) remove.push(p);
    set(
      wbRelsPart,
      text(wbRelsPart).replace(
        /<Relationship\b[^>]*\/(vbaProject|vbaProjectSignature|vbaProjectSignatureAgile|vbaProjectSignatureV3)"[^>]*\/>/g,
        ''
      )
    );
    set(
      '[Content_Types].xml',
      text('[Content_Types].xml')
        .replace(/<Override\b[^>]*PartName="\/xl\/vba[^"]*"[^>]*\/>/gi, '')
        .replace(/<Default\b[^>]*Extension="bin"[^>]*vbaProject[^>]*\/>/gi, '')
        .replace(/application\/vnd\.ms-excel\.sheet\.macroEnabled\.main\+xml/g, CT_XLSX)
    );
  }
  return { parts, remove };
}

/** A whole new workbook: from CSV, XLS, ODS, or nothing at all. */
/** `names`: [[name, 'Sheet!$A$1:$B$2'], …], the defined names the formulas use. */
export function freshXlsx(book, value, names = []) {
  const used = new Set();
  const sheetName = (n) => {
    let s =
        String(n || 'Sheet')
          .replace(/[[\]:*?/\\]/g, ' ')
          .replace(/^'+|'+$/g, '')
          .slice(0, 31)
          .trim() || 'Sheet',
      k = 1,
      base = s;
    while (used.has(s.toLowerCase())) s = base.slice(0, 27) + ' (' + ++k + ')';
    used.add(s.toLowerCase());
    return s;
  };
  const parts = {};
  const sheets = book.map((s, i) => {
    const { xml, dim } = sheetData(s, value, true);
    const cols = Object.keys(s.widths)
      .map(Number)
      .sort((a, b) => a - b)
      .map((c) => `<col min="${c + 1}" max="${c + 1}" width="${(s.widths[c] / 7.2).toFixed(2)}" customWidth="1"/>`)
      .join('');
    parts[`xl/worksheets/sheet${i + 1}.xml`] =
      '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
      + `<dimension ref="${dim}"/><sheetViews><sheetView workbookViewId="0"${i ? '' : ' tabSelected="1"'}/></sheetViews><sheetFormatPr defaultRowHeight="15"/>${cols ? '<cols>' + cols + '</cols>' : ''}${xml}</worksheet>`;
    return `<sheet name="${escAttr(sheetName(s.name))}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`;
  });
  const n = book.length;
  parts['[Content_Types].xml'] =
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
    + `<Override PartName="/xl/workbook.xml" ContentType="${CT_XLSX}"/>`
    + book
      .map(
        (_, i) =>
          `<Override PartName="/xl/worksheets/sheet${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`
      )
      .join('')
    + '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>';
  parts['_rels/.rels'] =
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>';
  parts['xl/workbook.xml'] =
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
    + `<workbookPr${epoch.base === 24107 ? ' date1904="1"' : ''}/><bookViews><workbookView/></bookViews><sheets>${sheets.join('')}</sheets>${names.length ? '<definedNames>' + names.map(([n, r]) => `<definedName name="${escAttr(n)}">${esc(r)}</definedName>`).join('') + '</definedNames>' : ''}<calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>`;
  parts['xl/_rels/workbook.xml.rels'] =
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    + book
      .map(
        (_, i) =>
          `<Relationship Id="rId${i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`
      )
      .join('')
    + `<Relationship Id="rId${n + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>`;
  parts['xl/styles.xml'] =
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy\\-mm\\-dd"/></numFmts>'
    + '<fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>'
    + '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
    + '<cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="4" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs>'
    + '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>';
  return { parts, remove: [] };
}
export { NotSupported };
