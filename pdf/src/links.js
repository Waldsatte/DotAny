// A web address a PDF links to: offered in the bar over the link and handed to the system's browser only when it is http
// or https, with nothing in it that could read as more than one address. Everything else a file names (file:,
// javascript:, mailto:, a program's own scheme) is never opened. The Rust side checks again (shared/rust/src/links.rs).
const WEB = /^https?:\/\/[^\s"<>`\\/][^\s"<>`\\]*$/i;
const CONTROL = /[\u0000-\u001f\u007f]/;

export function isWebAddress(url) {
  const u = String(url || '').trim();
  return u.length <= 4096 && WEB.test(u) && !CONTROL.test(u);
}
