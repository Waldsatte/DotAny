// Everything the page asks of the Rust side goes through here. These are the family's commands, from dotany-base;
// a tool's own commands go in its own src/backend.js, through the same `invoke`.
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export { invoke };
const label = window.__TAURI_INTERNALS__.metadata.currentWindow.label;
// events for this window, or for all windows
export const on = (name, fn) => listen(name, (e) => fn(e.payload), { target: { kind: 'WebviewWindow', label } });
export const onWebview = (name, fn) => listen(name, (e) => fn(e.payload), { target: { kind: 'Webview', label } });
export const fileSrc = convertFileSrc;

export const setting = (key, value) => invoke('set_setting', { key, value });
export const canOpen = (path) => invoke('can_open', { path });
export const pickOpen = (label) => invoke('pick_open', { label });
export const pickSave = (start, name, ext, label) => invoke('pick_save', { start, name, ext, label });
export const defaultFolder = () => invoke('default_folder');
export const neighbour = (step) => invoke('neighbour', { step });
export const closeDocument = () => invoke('close_document');
export const keepMine = () => invoke('keep_mine');
export const forgetRecent = (path) => invoke('forget_recent', { path });
export const setFrame = (dark) => invoke('set_frame', { dark });
export const windowControl = (action) => invoke('window_control', { action });
export const windowMenu = (x, y) => invoke('window_menu', { x, y });
export const setSnapButton = (x, y, w, h) => invoke('set_snap_button', { x, y, w, h });
export const setTitle = (title) => invoke('set_title', { title });
export const setMenuWords = (words) => invoke('set_menu_words', { words });
export const setEdited = (edited) => invoke('set_edited', { edited });
export const windowReady = () => invoke('window_ready');
export const startupDone = (js) => invoke('startup_done', { js });
export const closeWindow = () => invoke('close_window');
export const printWindow = () => invoke('print_window');
