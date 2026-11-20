// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';
import { Channel } from '@tauri-apps/api/core';

export const openFile = (path) => invoke('open_file', { path });
export const listTakes = () => invoke('list_takes');
export const deleteTakes = (paths) => invoke('delete_takes', { paths });
export const renameTake = (path, name) => invoke('rename_take', { path, name });
export const listMics = () => invoke('list_mics');
/** `onData` hears the new bars, the level and how far along, about twenty times a second. */
export function startRecording(name, mic, onData) {
  const ch = new Channel();
  ch.onmessage = onData;
  return invoke('start_recording', { name, mic, onData: ch });
}
export const pauseRecording = (paused) => invoke('pause_recording', { paused });
export const addMark = (at) => invoke('add_mark', { at });
export const stopRecording = () => invoke('stop_recording');
export const saveFile = (path, fmt, quality) => invoke('save_file', { path, fmt, quality });
/** A take of the list by its path, or with none the open one. */
export const openInAudio = (path) => invoke('open_in_audio', { path: path ?? null });
