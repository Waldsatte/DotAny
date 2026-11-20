// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

export const openFile = (path) => invoke('open_file', { path });
// the book being opened, as an ArrayBuffer, once the Rust side has found no lock on it
export const bookBytes = () => invoke('book_bytes');
export const bookOpened = (path) => invoke('book_opened', { path });
export const bookFailed = () => invoke('book_failed');
export const estimateSave = () => invoke('estimate_save');
export const saveFile = (path) => invoke('save_file', { path });
