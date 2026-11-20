// The rule: this program holds no decoder or encoder, and no library that could. Fails the build if the resolved dependency
// graph names one. Run on every build.
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const FORBIDDEN =
  /(ffmpeg|libav|x264|x265|openh264|h264|h265|hevc|vpx|dav1d|rav1e|gstreamer|^gst|mpv|vlc|symphonia|fdk|faad|opus|vorbis|minimp3|mp4parse|^mp4$|matroska)/i;

// both systems the program is built for: a crate only the Mac's build pulls in is checked as well as Windows'. cargo
// resolves the graph for a target whether or not that target is installed
const PLATFORMS = ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin'];

export function checkDeps() {
  const names = new Map();
  for (const platform of PLATFORMS) {
    const meta = JSON.parse(
      execFileSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', platform], {
        cwd: join(root, 'src-tauri'),
        encoding: 'utf8',
        maxBuffer: 2e8,
      })
    );
    const byId = new Map(meta.packages.map((p) => [p.id, p]));
    for (const n of meta.resolve.nodes) names.set(n.id, byId.get(n.id).name);
  }
  const bad = [...new Set(names.values())].filter((n) => FORBIDDEN.test(n));
  if (bad.length) throw new Error(`Forbidden codec in the build: ${bad.join(', ')}`);
  return { crates: names.size };
}
if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, '/')))
  console.log(JSON.stringify(checkDeps()));
