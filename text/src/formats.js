// What New can make, by kind: the family's panel asks which (shared/web/new-panel.js); `kind` is the view the
// new file opens in, `blank` what it starts with, `langId` its highlighting.
export const NEW_GROUPS = [
  {
    key: 'doc',
    label: '文档',
    items: [
      { id: 'docx', kind: 'doc', label: 'Word', ext: '.docx', also: '文档 document' },
      { id: 'md', kind: 'md', label: 'Markdown', ext: '.md', also: '文档 document' },
      { id: 'txt', kind: 'txt', label: '纯文本', ext: '.txt', also: 'text plain 文本 便签' },
    ],
  },
  {
    key: 'code',
    label: '代码',
    items: [
      { id: 'py', kind: 'python', label: 'Python', ext: '.py', also: '代码 code', langId: 'python' },
      { id: 'js', kind: 'plain', label: 'JavaScript', ext: '.js', also: '代码 code', langId: 'js' },
      { id: 'ts', kind: 'plain', label: 'TypeScript', ext: '.ts', also: '代码 code', langId: 'ts' },
      { id: 'html', kind: 'plain', label: 'HTML', ext: '.html', also: '代码 code 网页', langId: 'html' },
      { id: 'css', kind: 'plain', label: 'CSS', ext: '.css', also: '代码 code 样式', langId: 'css' },
    ],
  },
  {
    key: 'data',
    label: '数据与配置',
    items: [
      {
        id: 'json',
        kind: 'json',
        label: 'JSON',
        ext: '.json',
        also: '数据 配置 data config',
        blank: '{}\n',
        langId: 'json',
      },
      { id: 'yaml', kind: 'plain', label: 'YAML', ext: '.yaml', also: 'yml 数据 配置 data config', langId: 'yaml' },
      { id: 'toml', kind: 'plain', label: 'TOML', ext: '.toml', also: '数据 配置 data config' },
      { id: 'xml', kind: 'plain', label: 'XML', ext: '.xml', also: '数据 配置 data config', langId: 'xml' },
      { id: 'ini', kind: 'plain', label: 'INI', ext: '.ini', also: '数据 配置 data config' },
    ],
  },
  {
    key: 'sub',
    label: '字幕',
    items: [
      { id: 'srt', kind: 'plain', label: 'SRT', ext: '.srt', also: '字幕 subtitle' },
      { id: 'vtt', kind: 'plain', label: 'VTT', ext: '.vtt', also: '字幕 subtitle', blank: 'WEBVTT\n\n' },
    ],
  },
];

// What each kind of file can be saved as, for the family's save card: its own format first, then what it can become
const SAME = '保持当前文件格式。';
const WORD_TO_MD = {
  ext: '.md',
  label: 'Markdown',
  note: '仅保留标题与列表等基础结构，字体颜色、字号与行距将被移除。',
};
const TO_TXT = { ext: '.txt', label: '纯文本', note: '仅导出纯文字内容，图片、表格及所有排版样式将被移除。' };
export const FORMAT = {
  'doc.docx': { ext: '.docx', label: 'Word', note: SAME },
  'doc.md': { ...WORD_TO_MD },
  'doc.txt': { ...TO_TXT },
  'md.md': { ext: '.md', label: 'Markdown', note: SAME },
  'md.docx': { ext: '.docx', label: 'Word', note: '完整保留标题、列表与表格结构，可在 Word 中无缝继续编辑。' },
  'md.txt': { ...TO_TXT },
  'txt.txt': { ext: '.txt', label: '纯文本', note: SAME },
  'txt.md': { ext: '.md', label: 'Markdown', note: '文件内容保持不变，仅更改扩展名。' },
  'txt.docx': { ext: '.docx', label: 'Word', note: '按换行符将内容拆分为独立段落。' },
  json: { ext: '.json', label: 'JSON', note: SAME },
  python: { ext: '.py', label: 'Python', note: SAME },
  plain: { ext: '', label: '保留排版', note: SAME },
  log: { ext: '.log', label: '日志文件', note: SAME },
  // doc, rtf and odt are never written back: every choice is a new file
  'source.docx': { ext: '.docx', label: 'Word', note: '可编辑的新文档，原文件不变。' },
  'source.md': { ...WORD_TO_MD },
  'source.txt': { ...TO_TXT },
  // DOS text art (.nfo, .diz, .ans) is only viewed: saved as UTF-8 plain text, the original untouched
  'nfo.txt': { ext: '.txt', label: '纯文本', note: '以 UTF-8 编码保存，方框与色块字符原样保留。' },
  'ans.txt': { ext: '.txt', label: '纯文本', note: '仅导出字符，色彩将被移除。' },
};
export const KINDS = {
  doc: ['doc.docx', 'doc.md', 'doc.txt'],
  md: ['md.md', 'md.docx', 'md.txt'],
  txt: ['txt.txt', 'txt.md', 'txt.docx'],
  json: ['json'],
  python: ['python'],
  plain: ['plain'],
  log: ['log'],
  source: ['source.docx', 'source.md', 'source.txt'],
  nfo: ['nfo.txt'],
  ans: ['ans.txt'],
};
