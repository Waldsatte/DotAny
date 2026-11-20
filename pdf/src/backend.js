// This tool's own commands. The family's are in @dotany/web/backend.js.
import { invoke } from '@dotany/web/backend.js';

/** The PDF's bytes, whole, as an ArrayBuffer. Rejects with 'missing', 'refused' or 'damaged'. */
export const readPdf = (path) => invoke('read_pdf', { path });
/** The page has read it (with this password, if it needed one): now it's the open file. Answers what the file allows. */
export const adoptPdf = (path, password) => invoke('adopt_pdf', { path, password: password || null });
/** The system's open dialog for PDFs and pictures to add as pages: [{ id, name, kind: 'pdf' | 'image' | 'refused', size }] */
export const pickSources = (label) => invoke('pick_sources', { label });
export const addSources = (paths) => invoke('add_sources', { paths });
export const readSource = (id) => invoke('read_source', { id });
export const sourcePassword = (id, password) => invoke('source_password', { id, password });
/** A picture of a signature, chosen in the system's open dialog. Rejects with 'none' when nothing was chosen. */
export const pickPicture = (label) => invoke('pick_picture', { label });
/** Bytes the page drew for a save, kept under an id until the window lets go of the file. */
export const stash = (id, bytes) => invoke('stash', bytes, { headers: { id: encodeURIComponent(id) } });
export const dropStash = () => invoke('drop_stash');
export const putFont = (name, bytes) => invoke('put_font', bytes, { headers: { name } });
export const fontsHeld = () => invoke('fonts_held');
/** The file a plan makes: its size, then written. */
export const estimatePdf = (plan) => invoke('estimate_pdf', { plan: JSON.stringify(plan) });
export const savePdf = (path, plan) => invoke('save_pdf', { path, plan: JSON.stringify(plan) });
export const saveDocx = (path, book) => invoke('save_docx', { path, book: JSON.stringify(book) });
/** A picture of a page, written whole. */
export const saveBytes = (path, bytes) => invoke('save_bytes', bytes, { headers: { path: encodeURIComponent(path) } });
export const memory = () => invoke('memory');
/** A web address from the file, to the system's browser: http and https only, checked again on the Rust side. */
export const openWebLink = (url) => invoke('open_web_link', { url });
