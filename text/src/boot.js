// Text's Rust starts each window with { start: { path, format }, new } (app.rs, open_window); the family's shell reads
// start as the file to open and new as the kind to make (shell.js, begin). Read before the shell is made.
const boot = window.__DOTANY__;
if (boot && boot.start && typeof boot.start === 'object') {
  const { path, format } = boot.start;
  boot.start = path || null;
  if (!path && format) boot.new = format;
}
