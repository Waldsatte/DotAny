// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

export const openFile = (path) => invoke('open_file', { path });
// the file being opened, once the Rust side has looked at it: bytes (XLSX, XLSM), cells (XLS, ODS) or text (CSV, TSV)
export const sheetBytes = () => invoke('sheet_bytes');
export const sheetModel = () => invoke('sheet_model');
export const sheetText = (enc) => invoke('sheet_text', { enc });
export const sheetOpened = (path) => invoke('sheet_opened', { path });
export const sheetFailed = () => invoke('sheet_failed');
export const sheetNew = () => invoke('sheet_new');
export const saveXlsx = (path, parts, remove, fresh, exported) =>
  invoke('save_xlsx', { path, parts, remove, fresh, export: exported });
export const saveText = (path, text, enc, bom, exported) =>
  invoke('save_text', { path, text, enc, bom, export: exported });
