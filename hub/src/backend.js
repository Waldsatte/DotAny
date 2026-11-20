// DotAny's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

export const tools = () => invoke('hub_tools');
export const recents = () => invoke('hub_recents');
export const forget = (path) => invoke('hub_forget', { path });
export const clear = () => invoke('hub_clear');
export const open = (path, tool) => invoke('hub_open', { path, tool: tool || '' });
export const make = (tool, ext) => invoke('hub_new', { tool, ext: ext || '' });
export const pick = (label, many) => invoke('hub_pick', { label, many });
export const files = (paths) => invoke('hub_files', { paths });
export const targets = (files) => invoke('convert_targets', { files });
export const estimate = (files, target, o) => invoke('convert_estimate', { files, target, ...o });
export const convert = (files, target, o) => invoke('convert_run', { files, target, ...o });
export const autostart = (on) => (on == null ? invoke('autostart_get') : invoke('autostart_set', { on }));
export const shellMenu = (on, label) =>
  on == null ? invoke('shell_menu_get') : invoke('shell_menu_set', { on, label });
export const defaultApps = () => invoke('default_apps');
// the Mac: these kinds are opened by DotAny from now on (on), or by what opened them before; answers what the system says
export const defaultSet = (exts, on) => invoke('default_set', { exts, on });
// one tool's own page in Windows' Default apps (by key), or the whole list; it answers with the link it opened
export const openDefaultApps = (tool) => invoke('open_default_apps', { tool: tool || null });
// Send on the Contact page: 'sent', or why not ('offline', 'many', 'later', 'long', 'refused'); and the text a press
// could not send, kept until it is
export const feedback = (text, lang) => invoke('send_feedback', { text, lang });
export const feedbackDraft = () => invoke('feedback_draft');
export const waiting = (ready) => invoke('convert_waiting', { ready });
// Check for Updates and Install (src-tauri/src/update.rs): the state, a check, and getting the latest installer
// (for one tool not installed yet, or none for Update); the state also comes as the 'update' event
export const updateStatus = () => invoke('update_status');
export const updateCheck = () => invoke('update_check');
export const updateGet = (tool) => invoke('update_get', { tool: tool || null });
// the new version downloaded and checked by itself (state 'ready') goes in; and whether a newer
// version is downloaded by itself (Auto-download updates)
export const updateInstall = () => invoke('update_install');
export const autoDownload = (on) => (on == null ? invoke('auto_download_get') : invoke('auto_download_set', { on }));
