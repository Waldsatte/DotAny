// Builds the page into dist/. The code view and each language are separate files, loaded when first needed.
import { build } from 'esbuild';
import { generate } from './notices.mjs';
import { SHARED, bundleOptions } from '../../shared/scripts/build.mjs';
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist');
rmSync(dist, { recursive: true, force: true });
mkdirSync(join(dist, 'fonts'), { recursive: true });

const noFonts = process.env.DOTANY_NO_FONTS === '1';
console.log('notices', JSON.stringify(await generate()));
const result = await build({
  entryPoints: [join(root, 'src/main.js')],
  bundle: true,
  splitting: true,
  format: 'esm',
  minify: true,
  target: 'es2022',
  outdir: dist,
  charset: 'utf8',
  legalComments: 'none',
  metafile: true,
  chunkNames: 'chunks/[name]-[hash]',
  // @dotany/web is shared/web: the family's modules
  ...bundleOptions(root),
});
// the family's sheet first, then Text's own, which only adds
let css =
  readFileSync(join(SHARED, 'web/family.css'), 'utf8') + '\n\n' + readFileSync(join(root, 'src/styles.css'), 'utf8');
// the family's fonts aren't built in: every DotAny program reads the one shared copy (src-tauri/src/fonts.rs);
// the code view's font is Text's own
const fonts = ['JetBrainsMono-wght.ttf', 'unscii-16.woff'];
if (noFonts) css = css.replace(/@font-face \{[^}]*\}\n/g, '');
else for (const f of fonts) copyFileSync(join(root, 'fonts', f), join(dist, 'fonts', f));
writeFileSync(join(dist, 'styles.css'), css);
writeFileSync(
  join(dist, 'index.html'),
  `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>DotAny Text</title>
<link rel="stylesheet" href="styles.css">
${noFonts ? '' : '<link rel="preload" href="http://dotfont.localhost/StackSansText-wght.ttf" as="font" type="font/ttf" crossorigin>'}
</head>
<body>
<div id="app"></div>
<script type="module" src="main.js"></script>
</body>
</html>
`
);
const sizes = Object.entries(result.metafile.outputs).map(
  ([k, v]) => `${k.replace(/^.*dist[\\/]/, '')} ${(v.bytes / 1024).toFixed(0)}KB`
);
console.log(sizes.join('\n'));
