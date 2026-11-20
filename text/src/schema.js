// The document's shape: only what the toolbar groups can make. No DOM at import time, so tests can use it under Node.
import { Schema } from 'prosemirror-model';
import { tableNodes } from 'prosemirror-tables';

export const DEFAULT_SIZE = 17,
  DEFAULT_LH = 1.7;
// `pid` and `ppr`: which paragraph of a Word file this is, and its properties the toolbar doesn't know about, carried unread
const blockAttrs = { align: { default: null }, lh: { default: null }, pid: { default: null }, ppr: { default: '' } };
const readBlock = (dom) => ({ align: dom.style.textAlign || null, lh: parseFloat(dom.style.lineHeight) || null });
const blockStyle = (a) => {
  const s = (a.align ? `text-align:${a.align};` : '') + (a.lh ? `line-height:${a.lh};` : '');
  return s ? { style: s } : {};
};

// a picture's address stays as the file wrote it; only what the page loads goes through this
let showImage = (src) => src;
export const setImageResolver = (fn) => {
  showImage = fn;
};

// the toolbar's colours, and any other a Word file brings; black follows the theme so it stays readable at night
export const COLOR_HEX = { gray: '75716B', red: 'B3261E', blue: '1F4FA8', green: '2C6E49' };
const colorCss = (c) => {
  if (!c.startsWith('#')) return `var(--c-${c})`;
  const v = parseInt(c.slice(1), 16);
  return ((v >> 16) & 255) < 48 && ((v >> 8) & 255) < 48 && (v & 255) < 48 ? 'var(--text)' : c;
};

const baseTables = tableNodes({
  tableGroup: 'block',
  cellContent: 'block+',
  cellAttributes: { tcpr: { default: '' } },
});
const wordTables = {
  ...baseTables,
  table: { ...baseTables.table, attrs: { tid: { default: null }, tblpr: { default: '' }, grid: { default: '' } } },
  table_row: { ...baseTables.table_row, attrs: { trpr: { default: '' } } },
};

export const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: {
      group: 'block',
      content: 'inline*',
      attrs: blockAttrs,
      parseDOM: [{ tag: 'p', getAttrs: readBlock }],
      toDOM: (n) => ['p', blockStyle(n.attrs), 0],
    },
    heading: {
      group: 'block',
      content: 'inline*',
      defining: true,
      attrs: { ...blockAttrs, level: { default: 1 } },
      parseDOM: [1, 2, 3].map((level) => ({ tag: 'h' + level, getAttrs: (d) => ({ ...readBlock(d), level }) })),
      toDOM: (n) => ['h' + n.attrs.level, blockStyle(n.attrs), 0],
    },
    // how a Markdown file wrote its list (marker, first number, blank lines between items) travels with the list
    bullet_list: {
      group: 'block',
      content: 'list_item+',
      attrs: { marker: { default: '-' }, loose: { default: false }, num: { default: null }, key: { default: null } },
      parseDOM: [{ tag: 'ul' }],
      toDOM: () => ['ul', 0],
    },
    ordered_list: {
      group: 'block',
      content: 'list_item+',
      attrs: { start: { default: 1 }, loose: { default: false }, num: { default: null }, key: { default: null } },
      parseDOM: [{ tag: 'ol', getAttrs: (d) => ({ start: +d.getAttribute('start') || 1 }) }],
      toDOM: (n) => ['ol', n.attrs.start === 1 ? {} : { start: n.attrs.start }, 0],
    },
    list_item: { content: 'paragraph block*', defining: true, parseDOM: [{ tag: 'li' }], toDOM: () => ['li', 0] },
    image: {
      group: 'block',
      atom: true,
      draggable: true,
      attrs: {
        src: {},
        alt: { default: '' },
        iid: { default: null },
        rel: { default: null },
        w: { default: null },
        h: { default: null },
      },
      parseDOM: [
        {
          tag: 'img[src]',
          getAttrs: (d) => ({ src: d.dataset.src || d.getAttribute('src'), alt: d.getAttribute('alt') || '' }),
        },
      ],
      toDOM: (n) => ['img', { src: showImage(n.attrs.src), alt: n.attrs.alt, 'data-src': n.attrs.src }],
    },
    ...wordTables,
    // what a Word file holds that can't be edited here: shown, kept exactly, and never removed
    doc_locked: {
      group: 'block',
      atom: true,
      selectable: true,
      attrs: {
        lid: {},
        kind: { default: 'paragraph' },
        text: { default: '' },
        style: { default: null },
        rows: { default: null },
      },
      leafText: (n) => n.attrs.text || '',
      toDOM: (n) => {
        const cls = 'doc-locked' + (n.attrs.style ? ' ' + n.attrs.style : '');
        if (n.attrs.rows)
          return [
            'div',
            { class: cls },
            ['table', ['tbody', ...n.attrs.rows.map((r) => ['tr', ...r.map((c) => ['td', c])])]],
          ];
        return ['div', { class: cls }, n.attrs.text];
      },
    },
    text: { group: 'inline' },
    hard_break: {
      group: 'inline',
      inline: true,
      selectable: false,
      parseDOM: [{ tag: 'br' }],
      toDOM: () => ['br'],
      leafText: () => '\n',
    },
  },
  marks: {
    strong: { parseDOM: [{ tag: 'strong' }, { tag: 'b' }], toDOM: () => ['strong', 0] },
    em: { parseDOM: [{ tag: 'em' }, { tag: 'i' }], toDOM: () => ['em', 0] },
    underline: { parseDOM: [{ tag: 'u' }], toDOM: () => ['u', 0] },
    // run properties the toolbar doesn't know about, by their place in the document's table
    rx: {
      attrs: { id: {} },
      excludes: '',
      parseDOM: [{ tag: 'span[data-rx]', getAttrs: (d) => ({ id: +d.dataset.rx }) }],
      toDOM: (m) => ['span', { 'data-rx': m.attrs.id }, 0],
    },
    size: {
      attrs: { px: {} },
      parseDOM: [{ tag: 'span[data-size]', getAttrs: (d) => ({ px: +d.dataset.size }) }],
      toDOM: (m) => ['span', { 'data-size': m.attrs.px, style: `font-size:${m.attrs.px}px` }, 0],
    },
    color: {
      attrs: { c: {} },
      parseDOM: [{ tag: 'span[data-color]', getAttrs: (d) => ({ c: d.dataset.color }) }],
      toDOM: (m) => ['span', { 'data-color': m.attrs.c, style: `color:${colorCss(m.attrs.c)}` }, 0],
    },
  },
});
