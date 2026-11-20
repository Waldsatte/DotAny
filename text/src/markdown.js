// Markdown <-> the article view, under one rule: a file is never damaged.
//
// parseMarkdown returns null unless every part of the file is something the article view can really show (headings 1-3,
// one-line paragraphs, bold and italic, lists, simple tables, pictures alone on a line). Inline links and bare URLs are
// shown and edited as the text they are, character for character. Anything else (code blocks, quotes, reference links,
// HTML, front matter, hard line breaks and so on) sends the whole file to the code view.
//
// Each top-level block remembers the exact text it came from. Saving writes that text back for every block that is
// still the same, and new Markdown only for blocks that were edited. So unedited bytes stay what they were, whatever
// style the file was written in, and the tests check it.

import { schema } from './schema.js';

const N = schema.nodes,
  M = schema.marks;
class Foreign extends Error {}
const foreign = (why) => {
  throw new Foreign(why);
};

// ---------- one line of text ----------
// A link is kept as the exact characters that spell it: nothing inside one is read as formatting or ever escaped.
// CommonMark's inline link: [text](destination "title"), without nested brackets.
const LINK =
  /\[(?:[^[\]\\\n]|\\.)*\]\((?:<[^<>\n]*>|(?:[^\s()\\]|\\.|\((?:[^\s()\\]|\\.)*\))*)(?:[ \t]+(?:"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'))?\)/y;
const BARE_URL = /https?:\/\/[^\s<>[\]]+/y;
function tokenAt(s, i) {
  if (s[i] === '[') {
    LINK.lastIndex = i;
    const m = LINK.exec(s);
    return m ? m[0].length : 0;
  }
  if (s[i] === 'h') {
    BARE_URL.lastIndex = i;
    const m = BARE_URL.exec(s);
    return m ? m[0].length : 0;
  }
  return 0;
}
/** Where links and URLs are in a line, for showing them; escaped brackets are not links. */
export function linkRanges(s) {
  const out = [];
  for (let i = 0; i < s.length;) {
    if (s[i] === '\\') {
      i += 2;
      continue;
    }
    const n = tokenAt(s, i);
    if (n) {
      out.push([i, i + n]);
      i += n;
    } else i++;
  }
  return out;
}
const HOLE = '\uE000';
const withoutLinks = (s) => {
  let out = '',
    at = 0;
  for (const [a, b] of linkRanges(s)) {
    out += s.slice(at, a) + HOLE;
    at = b;
  }
  return out + s.slice(at);
};
const PUNCT = /[!-/:-@[-`{-~]/;
const WORD = /[\p{L}\p{N}]/u;
// things that would mean something the article view can't show
function checkInline(raw) {
  const s = withoutLinks(raw);
  if (s.replace(/\\./g, '').includes('!' + HOLE)) foreign('inline picture');
  if (s.replace(/\\./g, '').includes('`')) foreign('inline code');
  if (/!\[|\[[^\]]*\]\s*[([]|\[\^/.test(s.replace(/\\./g, ''))) foreign('reference link, image or footnote');
  if (/<[a-zA-Z/!?]/.test(s.replace(/\\./g, ''))) foreign('html or autolink');
  if (/&(#\d+|#[xX][0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);/.test(s.replace(/\\./g, ''))) foreign('entity');
  if (s.replace(/\\./g, '').includes('~~')) foreign('strikethrough');
  if (s.endsWith('\\') && !s.endsWith('\\\\')) foreign('hard break');
  const plain = s.replace(/\\./g, 'x');
  for (let i = plain.indexOf('_'); i >= 0; i = plain.indexOf('_', i + 1)) {
    let a = i;
    while (plain[a - 1] === '_') a--;
    let b = i;
    while (plain[b + 1] === '_') b++;
    const before = plain[a - 1],
      after = plain[b + 1];
    // inside a word it is just a character (snake_case); at a word's edge it would be emphasis
    if (!(before && after && WORD.test(before) && WORD.test(after))) {
      if ((after && !/\s/.test(after)) || (before && !/\s/.test(before))) foreign('underscore emphasis');
    }
    i = b;
  }
}

function runsOf(s) {
  // [text, strong, em] pieces, with escapes resolved
  const out = [];
  const push = (t, strong, em) => {
    if (t) {
      const l = out[out.length - 1];
      if (l && l[1] === strong && l[2] === em) l[0] += t;
      else out.push([t, strong, em]);
    }
  };
  const walk = (s, strong, em) => {
    let i = 0,
      text = '';
    while (i < s.length) {
      const c = s[i];
      if (c === '\\' && i + 1 < s.length && PUNCT.test(s[i + 1])) {
        text += s[i + 1];
        i += 2;
        continue;
      }
      const link = tokenAt(s, i);
      if (link) {
        text += s.slice(i, i + link);
        i += link;
        continue;
      }
      if (c !== '*') {
        text += c;
        i++;
        continue;
      }
      let n = 1;
      while (s[i + n] === '*') n++;
      const nextCh = s[i + n];
      const close = nextCh && !/\s/.test(nextCh) ? findClose(s, i + n, Math.min(n, 3)) : null;
      if (!close) {
        text += s.slice(i, i + n);
        i += n;
        continue;
      }
      push(text, strong, em);
      text = '';
      const inner = s.slice(i + n, close.at);
      const k = Math.min(n, 3);
      text = s.slice(i, i + n - k);
      push(text, strong, em);
      text = '';
      if (k === 3) walk(inner, true, true);
      else if (k === 2) walk(inner, true, em);
      else walk(inner, strong, true);
      i = close.at + k;
    }
    push(text, strong, em);
  };
  walk(s, false, false);
  return out;
}
function findClose(s, from, k) {
  for (let i = from; i < s.length;) {
    if (s[i] === '\\') {
      i += 2;
      continue;
    }
    const link = tokenAt(s, i);
    if (link) {
      i += link;
      continue;
    }
    if (s[i] !== '*') {
      i++;
      continue;
    }
    let n = 1;
    while (s[i + n] === '*') n++;
    const prev = s[i - 1];
    const closes = prev && !/\s/.test(prev);
    // a run of exactly this length ends it; a longer run ends it with its last stars
    if (closes && n === k) return { at: i };
    if (closes && n > k && n === 3) return { at: i + n - k };
    i += n;
  }
  return null;
}
function inlineNodes(s) {
  checkInline(s);
  const nodes = runsOf(s).map(([t, strong, em]) =>
    schema.text(t, [...(strong ? [M.strong.create()] : []), ...(em ? [M.em.create()] : [])])
  );
  // stars have many tricky readings; only the plain, unambiguous way of writing them is trusted
  if (s.includes('*') && inlineText(N.paragraph.create(null, nodes)) !== s) foreign('unusual stars');
  return nodes;
}

const LINK_LIKE = /\[[^\]]*\]\s*[([:]/;
function escapeText(t, { cell = false } = {}) {
  let out = '';
  for (let i = 0; i < t.length; i++) {
    const c = t[i],
      prev = t[i - 1],
      next = t[i + 1];
    const link = c === '\\' ? 0 : tokenAt(t, i);
    if (link) {
      out += t.slice(i, i + link);
      i += link - 1;
      continue;
    }
    if (c === '\\') out += next === undefined || PUNCT.test(next) ? '\\\\' : '\\';
    else if (c === '*') out += prev && next && /\s/.test(prev) && /\s/.test(next) ? '*' : '\\*';
    else if (c === '`') out += '\\`';
    else if (c === '[') out += LINK_LIKE.test(t.slice(i)) || next === '^' ? '\\[' : '[';
    else if (c === '!' && next === '[') out += '\\!';
    else if (c === '<') out += next && /[a-zA-Z/!?]/.test(next) ? '\\<' : '<';
    else if (c === '&') out += /^&(#\d+|#[xX][0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);/.test(t.slice(i)) ? '\\&' : '&';
    else if (c === '~' && (next === '~' || prev === '~')) out += '\\~';
    else if (c === '_')
      out +=
        prev && next && WORD.test(prev) && WORD.test(next)
          ? '_'
          : (prev && !/\s/.test(prev)) || (next && !/\s/.test(next))
            ? '\\_'
            : '_';
    else if (c === '|' && cell) out += '\\|';
    else out += c;
  }
  return out;
}
const textStart = (s) =>
  s
    .replace(/^(#{1,6})(?=\s|$)/, '\\$1')
    .replace(/^([-+])(?=\s|$)/, '\\$1')
    .replace(/^(\d{1,9})([.)])(?=\s|$)/, '$1\\$2')
    .replace(/^>/, '\\>')
    .replace(/^(-{3,}|=+)\s*$/, '\\$1');

function inlineText(node, opts) {
  const open = [];
  let out = '';
  const closeTo = (keep) => {
    while (open.length > keep) out += open.pop();
  };
  const delims = (child) => [
    ...(child && M.strong.isInSet(child.marks) ? ['**'] : []),
    ...(child && M.em.isInSet(child.marks) ? ['*'] : []),
  ];
  const common = (want) => {
    let n = 0;
    while (n < open.length && want.includes(open[n])) n++;
    return n;
  };
  const kids = [];
  node.forEach((child) => kids.push(child));
  kids.forEach((child, i) => {
    const want = delims(child);
    // a mark can't start or end against a space, so spaces at a mark's edge stay outside it
    const m = /^(\s*)(.*?)(\s*)$/s.exec(child.isText ? child.text : '');
    closeTo(common(want));
    out += m[1];
    if (m[2]) {
      want
        .filter((d) => !open.includes(d))
        .forEach((d) => {
          out += d;
          open.push(d);
        });
      out += escapeText(m[2], opts);
    }
    if (m[3]) {
      closeTo(common(delims(kids[i + 1])));
      out += m[3];
    }
  });
  closeTo(0);
  return out;
}
const lineOf = (node, opts) => textStart(inlineText(node, opts).trim());

// ---------- blocks: new Markdown for what was edited ----------
function cellText(cell) {
  const parts = [];
  cell.forEach((b) => {
    if (b.isTextblock) parts.push(inlineText(b, { cell: true }).trim());
    else parts.push(plain(b));
  });
  return parts.join('<br>');
}
const plain = (n) => n.textContent.replace(/\|/g, '\\|');
function blockText(node, indent = '') {
  if (node.type === N.paragraph) return lineOf(node);
  if (node.type === N.heading) return '#'.repeat(node.attrs.level) + ' ' + inlineText(node).trim();
  if (node.type === N.image) return `![${escapeText(node.attrs.alt)}](${node.attrs.src})`;
  if (node.type === N.bullet_list || node.type === N.ordered_list) {
    const ordered = node.type === N.ordered_list,
      lines = [];
    node.forEach((item, _o, i) => {
      const mark = ordered ? `${node.attrs.start + i}. ` : `${node.attrs.marker} `;
      const pad = indent + ' '.repeat(mark.length);
      let first = true,
        body = [];
      item.forEach((b) => {
        const text = blockText(b, pad);
        if (first) {
          body.push(indent + mark + text.replace(/^\[([ xX])\]/, '\\[$1]'));
          first = false;
        } else if (b.type === N.bullet_list || b.type === N.ordered_list) body.push(text);
        else
          body.push(
            text
              .split('\n')
              .map((l) => pad + l)
              .join('\n')
          );
      });
      lines.push(body.join('\n'));
    });
    return lines.join(node.attrs.loose ? '\n\n' : '\n');
  }
  if (node.type === N.table) {
    const rows = [];
    node.forEach((row) => {
      const cells = [];
      row.forEach((c) => cells.push(cellText(c)));
      rows.push(cells);
    });
    const line = (cells) => '| ' + cells.join(' | ') + ' |';
    const [head, ...rest] = rows;
    return [line(head), line(head.map(() => '---')), ...rest.map(line)].join('\n');
  }
  return node.textContent;
}

// ---------- reading blocks ----------
const LIST = /^(?:([-*+])|(\d{1,9})\.) (.*)$/;
const DELIM = /^\|?\s*-+\s*(\|\s*-+\s*)*\|?$/;
function splitRow(line) {
  let s = line.trim();
  if (s.startsWith('|')) s = s.slice(1);
  if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
  const cells = [];
  let cur = '';
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '\\' && i + 1 < s.length) {
      cur += s[i] + s[i + 1];
      i++;
      continue;
    }
    if (s[i] === '|') {
      cells.push(cur);
      cur = '';
      continue;
    }
    cur += s[i];
  }
  cells.push(cur);
  return cells.map((c) => c.trim());
}
function checkLine(l) {
  if (/\s$/.test(l)) foreign('trailing space');
  if (/^\s/.test(l)) foreign('indented text');
  if (/^(>|```|~~~|<|:::|\$\$)/.test(l)) foreign('block construct');
  if (/^(#{4,6})(\s|$)/.test(l)) foreign('deep heading');
  if (/^([-*_])\s*(\1\s*){2,}$/.test(l)) foreign('thematic break');
  if (/^=+$/.test(l) || /^-+$/.test(l)) foreign('setext underline');
  if (/^\[[^\]]+\]:/.test(l)) foreign('link definition');
  if (/^\d{1,9}\)\s/.test(l)) foreign('other list marker');
}
const isBlank = (l) => l === '';

function parseList(lines, start, indent) {
  const first = lines[start].slice(indent).match(LIST);
  if (!first) foreign('list expected');
  const ordered = !first[1];
  const marker = first[1];
  const items = [];
  let i = start,
    loose = null;
  for (;;) {
    const m = lines[i].slice(indent).match(LIST);
    const text = m[3];
    if (/^\[[ xX]\]/.test(text)) foreign('task list');
    if (/\s$/.test(text) || /^\s/.test(text) || !text) foreign('list item spacing');
    const contentIndent = indent + (lines[i].length - indent - text.length);
    const kids = [N.paragraph.create(null, inlineNodes(text))];
    i++;
    while (
      i < lines.length
      && lines[i].startsWith(' '.repeat(contentIndent))
      && lines[i][contentIndent] !== ' '
      && !isBlank(lines[i])
    ) {
      if (!LIST.test(lines[i].slice(contentIndent))) foreign('item continuation');
      const nested = parseList(lines, i, contentIndent);
      kids.push(nested.node);
      i = nested.end;
    }
    items.push(N.list_item.create(null, kids));
    if (i >= lines.length) break;
    if (!isBlank(lines[i])) {
      const peek = lines[i].startsWith(' '.repeat(indent)) ? lines[i].slice(indent).match(LIST) : null;
      if (peek && lines[i][indent] !== ' ') {
        if (ordered ? !!peek[1] : peek[1] !== marker) foreign('list changes marker');
        if (loose === true) foreign('mixed list spacing');
        loose = false;
        continue;
      }
      if (indent > 0 && !lines[i].startsWith(' '.repeat(indent))) break;
      if (indent > 0 && LIST.test(lines[i].trimStart())) break;
      foreign('text right after a list');
    }
    // one blank line, then another item of the same list: a loose list
    let j = i;
    while (j < lines.length && isBlank(lines[j])) j++;
    const again =
      j < lines.length && lines[j].startsWith(' '.repeat(indent)) && lines[j][indent] !== ' '
        ? lines[j].slice(indent).match(LIST)
        : null;
    const same = again && (ordered ? !again[1] : again[1] === marker);
    if (same) {
      if (j - i !== 1 || indent > 0 || loose === false) foreign('mixed list spacing');
      loose = true;
      i = j;
      continue;
    }
    if (j < lines.length && /^ /.test(lines[j])) foreign('indented text after a list');
    break;
  }
  const attrs = ordered ? { start: +first[2], loose: !!loose } : { marker, loose: !!loose };
  return { node: (ordered ? N.ordered_list : N.bullet_list).create(attrs, items), end: i };
}

function parseBlock(lines, i) {
  const l = lines[i];
  checkLine(l);
  const heading = l.match(/^(#{1,3}) (.+)$/);
  if (heading) {
    if (/\s#+$/.test(heading[2]) || /^\s/.test(heading[2])) foreign('heading with closing marks');
    return { node: N.heading.create({ level: heading[1].length }, inlineNodes(heading[2])), end: i + 1 };
  }
  if (/^#{1,3}$/.test(l)) foreign('empty heading');
  if (LIST.test(l)) return parseList(lines, i, 0);
  const pic = l.match(/^!\[([^\]\\]*)\]\(([^()\s]+)\)$/);
  if (pic) {
    if (/^(https?:)?\/\//i.test(pic[2])) foreign('remote picture');
    if (i + 1 < lines.length && !isBlank(lines[i + 1])) foreign('text right after a picture');
    return { node: N.image.create({ alt: pic[1], src: pic[2] }), end: i + 1 };
  }
  if (l.includes('|') && i + 1 < lines.length && DELIM.test(lines[i + 1]) && lines[i + 1].includes('-')) {
    const head = splitRow(l),
      width = head.length;
    if (splitRow(lines[i + 1]).length !== width) foreign('table shape');
    const cell = (type, s) => type.create(null, N.paragraph.create(null, inlineNodes(s)));
    const rows = [
      N.table_row.create(
        null,
        head.map((c) => cell(N.table_header, c))
      ),
    ];
    let j = i + 2;
    for (; j < lines.length && !isBlank(lines[j]); j++) {
      if (!lines[j].includes('|') || /\s$/.test(lines[j])) foreign('table row');
      const cells = splitRow(lines[j]);
      if (cells.length !== width) foreign('table shape');
      rows.push(
        N.table_row.create(
          null,
          cells.map((c) => cell(N.table_cell, c))
        )
      );
    }
    return { node: N.table.create(null, rows), end: j };
  }
  if (DELIM.test(l) && l.includes('-')) foreign('table delimiter');
  if (i + 1 < lines.length && !isBlank(lines[i + 1])) foreign('paragraph of several lines');
  return { node: N.paragraph.create(null, inlineNodes(l)), end: i + 1 };
}

const keyOf = (node) => JSON.stringify(node.toJSON());

/** { doc, sources } for the article view, or null: this file belongs in the code view. */
export function parseMarkdown(src) {
  if (src.includes('\r') || src.includes('\0')) return null;
  try {
    const lines = src.split('\n');
    let i = 0;
    while (i < lines.length && isBlank(lines[i])) i++;
    if (i === lines.length)
      return { doc: N.doc.create(null, N.paragraph.create()), sources: { head: src, tail: '', blocks: new Map() } };
    if (lines[i] === '---' && i === 0) return null;
    const head = lines.slice(0, i).join('\n') + (i ? '\n' : '');
    const nodes = [],
      blocks = new Map();
    let tail = '';
    while (i < lines.length) {
      const { node, end } = parseBlock(lines, i);
      const text = lines.slice(i, end).join('\n');
      let j = end;
      while (j < lines.length && isBlank(lines[j])) j++;
      // what stands between this block and the next, or ends the file
      const gap = j < lines.length ? '\n'.repeat(j - end + 1) : '';
      if (j >= lines.length) tail = '\n'.repeat(Math.max(0, lines.length - end));
      // new Markdown for this block must mean the same thing, or editing it later could change the file's meaning
      const again = parseBlock(blockText(node).split('\n'), 0);
      if (!again.node.eq(node)) return null;
      const key = keyOf(node);
      if (!blocks.has(key)) blocks.set(key, []);
      blocks.get(key).push({ text, gap });
      nodes.push(node);
      i = j;
    }
    const result = { doc: N.doc.create(null, nodes), sources: { head, tail, blocks } };
    // the last word: nothing goes in that doesn't come out exactly as it was
    return serializeMarkdown(result.doc, result.sources) === src ? result : null;
  } catch (e) {
    if (e instanceof Foreign) return null;
    throw e;
  }
}

/** The file's text: original text for every unchanged block, new Markdown for the rest. */
export function serializeMarkdown(doc, sources) {
  const used = new Map();
  const parts = [];
  doc.forEach((node) => {
    if (node.type === N.paragraph && node.content.size === 0) return;
    const key = keyOf(node),
      list = sources && sources.blocks.get(key),
      n = used.get(key) || 0;
    used.set(key, n + 1);
    const known = list && list[Math.min(n, list.length - 1)];
    parts.push(known && n < list.length ? known : { text: blockText(node), gap: '\n\n' });
  });
  if (!parts.length) return sources ? sources.head + sources.tail : '';
  const head = sources ? sources.head : '';
  // a file that was empty ends its first line properly once it has one
  const tail = sources && (sources.tail || sources.blocks.size) ? sources.tail : '\n';
  return head + parts.map((p, i) => p.text + (i < parts.length - 1 ? p.gap || '\n\n' : '')).join('') + tail;
}

// ---------- plain text: one paragraph per line ----------
export function parseText(text) {
  return N.doc.create(
    null,
    text.split('\n').map((line) => N.paragraph.create(null, line ? schema.text(line) : null))
  );
}
export function serializeText(doc) {
  const out = [];
  doc.forEach((node) => out.push(node.textContent));
  return out.join('\n');
}
