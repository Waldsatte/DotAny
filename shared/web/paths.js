// Paths as the system gives them, without assuming which system: the separator comes from the path itself.
const sepOf = (p) => (p && p.includes('\\') ? '\\' : '/');
export const dirOf = (path) => (path ? path.replace(/[\\/][^\\/]*$/, '') : null);
export const baseName = (path) => path.replace(/^.*[\\/]/, '');
export const isAbsolute = (p) => /^([a-zA-Z]:[\\/]|[\\/])/.test(p);
// A path to another computer or a device: `\\host\share`, `//host/share`, `\\?\…`, `\\.\…`, `\??\…`. Windows connects to
// that server, and signs in to it, as soon as anything looks at such a path.
export const isRemotePath = (p) => /^([\\/]{2}|[\\/]\?\?[\\/])/.test(p);
// `server\share` of a path on a network share, lower-cased, in either slash; null for anything else, device and verbatim
// paths included. The server part is whole: `nas@SSL` and `nas@80` are servers of their own.
export function shareOf(p) {
  const m = /^[\\/]{2}([^\\/]+)[\\/]([^\\/]+)/.exec(p || '');
  if (!m || m[1] === '?' || m[1] === '.' || m[2] === '.' || m[2] === '..') return null;
  return (m[1] + '\\' + m[2]).toLowerCase();
}
// The same share, as the one a document was opened from: a picture there reveals nothing new.
export const onSameShare = (a, b) => shareOf(a) !== null && shareOf(a) === shareOf(b);
export function joinPath(dir, name) {
  const sep = sepOf(dir);
  const clean = sep === '\\' ? name.replace(/\//g, '\\') : name;
  return dir && (dir.endsWith('/') || dir.endsWith('\\')) ? dir + clean : (dir || '') + sep + clean;
}
