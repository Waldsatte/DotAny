// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

export const openFile = (path) => invoke('open_file', { path });
export const estimateSave = () => invoke('estimate_save');
export const saveFile = (path) => invoke('save_file', { path });
