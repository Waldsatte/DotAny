// A Word file on one side, the article view on the other. Rust reads and writes the file; here its blocks become
// the editor's document and back. Blocks that come back unchanged are written as their original bytes, so this must
// give back exactly what it was given for anything not edited. Pure, so it can be tested under Node.
import { schema, COLOR_HEX } from './schema.js';

const N = schema.nodes,
  M = schema.marks;
const HEX_NAME = Object.fromEntries(Object.entries(COLOR_HEX).map(([k, v]) => [v, k]));

function inline(runs) {
  const out = [];
  for (const run of runs || []) {
    const marks = [];
    if (run.b) marks.push(M.strong.create());
    if (run.i) marks.push(M.em.create());
    if (run.u) marks.push(M.underline.create());
    if (run.sz != null) marks.push(M.size.create({ px: (run.sz * 2) / 3 }));
    if (run.color != null) marks.push(M.color.create({ c: HEX_NAME[run.color] || '#' + run.color }));
    if (run.extra != null) marks.push(M.rx.create({ id: run.extra }));
    const pieces = run.t.split('\n');
    pieces.forEach((piece, i) => {
      if (piece) out.push(schema.text(piece, marks));
      if (i < pieces.length - 1) out.push(N.hard_break.create(null, null, marks));
    });
  }
  return out;
}

const blockAttrs = (p) => ({
  align: p.align || null,
  lh: p.line != null ? p.line / 240 : null,
  pid: p.id ?? null,
  ppr: p.ppr || '',
});
function textblock(p) {
  const attrs = blockAttrs(p);
  return p.style
    ? N.heading.create({ ...attrs, level: +p.style[1] }, inline(p.runs))
    : N.paragraph.create(attrs, inline(p.runs));
}

// list paragraphs nest by level; a change of kind or number starts another list at that level
function nest(paras, from, level) {
  const lists = [];
  let i = from;
  while (i < paras.length && paras[i].list && paras[i].list.level >= level) {
    const head = paras[i].list;
    const type = head.kind === 'bullet' ? N.bullet_list : N.ordered_list;
    const items = [];
    while (
      i < paras.length
      && paras[i].list
      && paras[i].list.level >= level
      && (paras[i].list.level > level || (paras[i].list.kind === head.kind && paras[i].list.num === head.num))
    ) {
      const kids = [textblock(paras[i])];
      i++;
      if (i < paras.length && paras[i].list && paras[i].list.level > level) {
        const inner = nest(paras, i, level + 1);
        kids.push(...inner.lists);
        i = inner.next;
      }
      items.push(N.list_item.create(null, kids));
    }
    lists.push(type.create({ num: head.num ?? null }, items));
  }
  return { lists, next: i };
}

const lockedLabel = (b, T) =>
  b.text || (b.kind === 'picture' ? T('图像') : b.rows && b.rows.length ? '' : T('暂不支持渲染的元素'));

export function blocksToDoc(blocks, T = (s) => s) {
  const out = [];
  for (let i = 0; i < blocks.length;) {
    const b = blocks[i];
    if (b.type === 'para' && b.list) {
      const run = [];
      let j = i;
      while (j < blocks.length && blocks[j].type === 'para' && blocks[j].list) run.push(blocks[j++]);
      out.push(...nest(run, 0, 0).lists);
      i = j;
      continue;
    }
    i++;
    if (b.type === 'para') out.push(textblock(b));
    else if (b.type === 'image')
      out.push(
        N.image.create({ src: b.src || 'docx:' + b.rel, alt: b.alt || '', iid: b.id ?? null, rel: b.rel ?? null })
      );
    else if (b.type === 'locked')
      out.push(
        N.doc_locked.create({
          lid: b.id,
          kind: b.kind,
          text: lockedLabel(b, T),
          style: b.style || null,
          rows: b.rows && b.rows.length ? b.rows : null,
        })
      );
    else if (b.type === 'table') {
      const rows = b.rows.map((r) =>
        N.table_row.create(
          { trpr: r.trpr || '' },
          r.cells.map((c) =>
            (c.head ? N.table_header : N.table_cell).create({ tcpr: c.tcpr || '' }, c.paras.map(textblock))
          )
        )
      );
      out.push(N.table.create({ tid: b.id ?? null, tblpr: b.tblpr || '', grid: b.grid || '' }, rows));
    }
  }
  if (!out.length) out.push(N.paragraph.create());
  return N.doc.create(null, out);
}

function runsOf(block) {
  const runs = [];
  const add = (t, marks) => {
    const run = { t };
    for (const m of marks) {
      if (m.type === M.strong) run.b = true;
      else if (m.type === M.em) run.i = true;
      else if (m.type === M.underline) run.u = true;
      else if (m.type === M.size) run.sz = Math.round(m.attrs.px * 1.5);
      else if (m.type === M.color) run.color = COLOR_HEX[m.attrs.c] || m.attrs.c.replace(/^#/, '');
      else if (m.type === M.rx) run.extra = m.attrs.id;
    }
    const last = runs[runs.length - 1];
    if (last && ['b', 'i', 'u', 'sz', 'color', 'extra'].every((k) => last[k] === run[k])) last.t += t;
    else runs.push(run);
  };
  block.forEach((child) => {
    if (child.isText) add(child.text, child.marks);
    else if (child.type === N.hard_break) add('\n', child.marks);
  });
  return runs;
}

function paraOf(node, list) {
  const a = node.attrs,
    p = { type: 'para', runs: runsOf(node) };
  if (a.pid != null) p.id = a.pid;
  if (node.type === N.heading) p.style = 'h' + a.level;
  if (a.align) p.align = a.align;
  if (a.lh != null) p.line = Math.round(a.lh * 240);
  if (list) p.list = list;
  if (a.ppr) p.ppr = a.ppr;
  return p;
}

function plainPara(node) {
  const { type, ...p } = paraOf(node, null);
  return p;
}

function flatCell(node) {
  // a cell holds paragraphs; anything nested in one reads as its paragraphs
  const out = [];
  node.descendants((n) => {
    if (n.isTextblock) {
      out.push(plainPara(n));
      return false;
    }
    return true;
  });
  return out.length ? out : [{ runs: [] }];
}

/** The document as blocks. `newKeys` numbers lists that have no number in the file yet. */
export function docToBlocks(doc) {
  const out = [];
  let nextKey = 1;
  const walkList = (node, level) => {
    const kind = node.type === N.bullet_list ? 'bullet' : 'ordered';
    const ref = { kind, level };
    if (node.attrs.num != null) ref.num = node.attrs.num;
    else ref.new = nextKey++;
    node.forEach((item) => {
      let first = true;
      item.forEach((kid) => {
        if (first && kid.isTextblock) {
          out.push(paraOf(kid, { ...ref }));
          first = false;
        } else if (kid.type === N.bullet_list || kid.type === N.ordered_list) walkList(kid, level + 1);
        else block(kid);
      });
    });
  };
  const block = (n) => {
    if (n.isTextblock) out.push(paraOf(n, null));
    else if (n.type === N.bullet_list || n.type === N.ordered_list) walkList(n, 0);
    else if (n.type === N.image) {
      const b = { type: 'image', alt: n.attrs.alt };
      if (n.attrs.iid != null) b.id = n.attrs.iid;
      if (n.attrs.rel != null) b.rel = n.attrs.rel;
      else b.src = n.attrs.src;
      if (n.attrs.w) {
        b.width = n.attrs.w;
        b.height = n.attrs.h;
      }
      out.push(b);
    } else if (n.type === N.doc_locked) out.push({ type: 'locked', id: n.attrs.lid, kind: n.attrs.kind });
    else if (n.type === N.table) {
      const b = { type: 'table', rows: [] };
      if (n.attrs.tid != null) b.id = n.attrs.tid;
      if (n.attrs.tblpr) b.tblpr = n.attrs.tblpr;
      if (n.attrs.grid) b.grid = n.attrs.grid;
      n.forEach((row) => {
        const r = { cells: [] };
        if (row.attrs.trpr) r.trpr = row.attrs.trpr;
        row.forEach((cell) => {
          const c = { paras: flatCell(cell) };
          if (cell.attrs.tcpr) c.tcpr = cell.attrs.tcpr;
          if (cell.type === N.table_header) c.head = true;
          r.cells.push(c);
        });
        b.rows.push(r);
      });
      out.push(b);
      // a cell keeps words only; a picture that got into one, by a paste say, comes out after the table and isn't lost
      n.descendants((kid) => {
        if (kid.type === N.image) block(kid);
        return true;
      });
    }
  };
  doc.forEach(block);
  return out;
}

/** After a save: lists that got numbers keep them, so the next save doesn't make new ones. */
export function assignListNumbers(state, assigned) {
  if (!assigned || !assigned.length) return null;
  const numbers = new Map(assigned);
  const tr = state.tr;
  let nextKey = 1;
  state.doc.descendants((node, pos) => {
    if (node.type === N.table) return false;
    if ((node.type === N.bullet_list || node.type === N.ordered_list) && node.attrs.num == null) {
      const key = nextKey++;
      if (numbers.has(key)) tr.setNodeMarkup(pos, null, { ...node.attrs, num: numbers.get(key) });
    }
    return true;
  });
  return tr.docChanged ? tr.setMeta('addToHistory', false) : null;
}

/** Plain words, for saving as text: one line per paragraph, table cells separated by tabs. */
export function docToPlain(doc) {
  const lines = [];
  const walk = (n) => {
    if (n.isTextblock) {
      lines.push(n.textBetween(0, n.content.size, '', (leaf) => (leaf.type === N.hard_break ? '\n' : '')));
      return;
    }
    if (n.type === N.table) {
      n.forEach((row) => {
        const cells = [];
        row.forEach((c) => cells.push(c.textContent));
        lines.push(cells.join('\t'));
      });
      return;
    }
    if (n.type === N.doc_locked) {
      if (n.attrs.rows) n.attrs.rows.forEach((r) => lines.push(r.join('\t')));
      else lines.push(n.attrs.text);
      return;
    }
    if (n.type === N.image) return;
    n.forEach(walk);
  };
  doc.forEach(walk);
  return lines.join('\n') + '\n';
}
