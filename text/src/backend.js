// Everything the page asks of the Rust side: the family's commands (shared/web/backend.js, from dotany-base), then Text's
// own. A Text window keeps its own document, so opening and the open dialog are Text's.
import { invoke } from '@dotany/web/backend.js';

export * from '@dotany/web/backend.js';

export const windowLabel = window.__TAURI_INTERNALS__.metadata.currentWindow.label;

export const openPath = (path) => invoke('open_path', { path });
export const pickOpen = () => invoke('pick_open');
export const newDocument = () => invoke('new_document');
export const newWordDocument = () => invoke('new_word_document');
export const saveWord = (blocks) => invoke('save_word', { blocks });
export const saveWordAs = (path, blocks, overwrite) => invoke('save_word_as', { path, blocks, overwrite });
export const exportWord = (path, blocks, overwrite, base) => invoke('export_word', { path, blocks, overwrite, base });
export const saveConvertedAs = (path, overwrite) => invoke('save_converted_as', { path, overwrite });
export const exportText = (path, text, overwrite) => invoke('export_text', { path, text, overwrite });
/** Word as Markdown: its pictures written beside it under `<name> 图1.png`… and linked from the text. */
export const exportMarkdown = (path, text, pictures, word, overwrite) =>
  invoke('export_markdown', { path, text, pictures, word, overwrite });
export const reopenAs = (name) => invoke('reopen_as', { label: name });
export const reloadDocument = () => invoke('reload_document');
export const saveDocument = (text, encoding, eol) => invoke('save_document', { text, encoding, eol });
export const saveDocumentAs = (path, text, overwrite, encoding, eol) =>
  invoke('save_document_as', { path, text, overwrite, encoding, eol });
export const copyLongAs = (path, overwrite) => invoke('copy_long_as', { path, overwrite });
export const allowImages = (paths) => invoke('allow_images', { paths });
export const newWindow = (format, path) => invoke('new_window', { format, path });
export const long = {
  lines: (start, count) => invoke('long_lines', { start, count }),
  state: () => invoke('long_state'),
  filter: (level) => invoke('long_filter', { level }),
  find: (seq, query, matchCase, wholeWord, regex) => invoke('long_find', { seq, query, matchCase, wholeWord, regex }),
  step: (from, forward, current) => invoke('long_find_step', { from, forward, current }),
  text: () => invoke('long_text'),
};
