// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

export const openFile = (path, password = null) => invoke('open_file', { path, password });
export const forgetSource = () => invoke('forget_source');
export const statPaths = (paths) => invoke('stat_paths', { paths });
export const pickFiles = (start) => invoke('pick_files', { start });
export const pickFolder = (start) => invoke('pick_folder', { start });
/** An entry's first `limit` bytes as an ArrayBuffer; throws 'password', 'unsupported' or 'damaged'. */
export const readEntry = (i, password, limit) => invoke('read_entry', { i, password, limit });
export const readLocal = (path, limit) => invoke('read_local', { path, limit });
export const tryPassword = (i, password) => invoke('try_password', { i, password });
export const extractTo = (nodes, location, folder, password) =>
  invoke('extract_to', { nodes, location, folder, password });
export const cancelJob = () => invoke('cancel_job');
export const saveArchive = (path, format, level, password, readPassword, nodes) =>
  invoke('save_archive', { path, format, level, password, readPassword, nodes });
export const estimateSave = (format, level, nodes) => invoke('estimate_save', { format, level, nodes });
export const checkArchive = (password) => invoke('check_archive', { password });
export const openEntry = (o) => invoke('open_entry', o);
export const siblings = () => invoke('siblings');
export const reveal = (path) => invoke('reveal', { path });
export const shellMenu = () => invoke('shell_menu');
export const setShellMenu = (on, openLabel, packLabel) => invoke('set_shell_menu', { on, openLabel, packLabel });
