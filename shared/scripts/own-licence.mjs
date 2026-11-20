// The program's own licence, printed at the top of its licence page, and the source lines for the libraries under the
// Mozilla Public License. Used by notices.mjs here and by text/scripts/notices.mjs.
//
// The family's code is under the MIT licence: the repository's LICENSE, up to its list of exceptions. A program with a
// LICENSE of its own (Books: GPL-3.0-or-later, because DjVu.js inside it is GPL) is under that one, and the GPL asks a
// program with a user interface to say so, that there is no warranty, and where its source is (GPL-3, sections 0 and
// 5 d). The MPL asks that whoever receives the program is told where the source of those libraries is (MPL-2.0, 3.2 a).
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = join(dirname(fileURLToPath(import.meta.url)), '../..');
export const SOURCE = 'https://github.com/Waldsatte/DotAny';
const LINE = '-'.repeat(80);

const read = (f) => readFileSync(f, 'utf8').replace(/\r\n/g, '\n').trim();

/**
 * root: the program's folder; name: its name as the About page shows it. Gives { licence, text }: the SPDX name for the
 * licence row, and the text for the top of the page, ending in the line that separates it from what follows.
 */
export function ownLicence({ root, name }) {
  const family = read(join(REPO, 'LICENSE'));
  const copyright = family.match(/^Copyright .*$/m)[0];
  const own = join(root, 'LICENSE');
  if (existsSync(own)) {
    const gpl = read(own);
    if (!/GNU GENERAL PUBLIC LICENSE\s+Version 3, 29 June 2007/.test(gpl))
      throw new Error(`${own}: a program's own LICENSE is expected to be the GPL, version 3`);
    const notice = [
      name,
      copyright,
      `${name} is free software: you can redistribute it and/or modify it under\nthe terms of the GNU General Public License as published by the Free Software\nFoundation, either version 3 of the License, or (at your option) any later\nversion.`,
      `${name} is distributed in the hope that it will be useful, but WITHOUT\nANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS\nFOR A PARTICULAR PURPOSE. See the GNU General Public License below for more\ndetails.`,
      `Source code: ${SOURCE} (the folder ${basename(root)}/)`,
      gpl,
    ];
    return { licence: 'GPL-3.0-or-later', text: notice.join('\n\n') + `\n\n${LINE}\n` };
  }
  // the MIT text alone, without the exceptions that follow it in the repository's LICENSE
  const mit = family.split(/\n-{20,}\n/)[0].trim();
  return { licence: 'MIT', text: [name, mit, `Source code: ${SOURCE}`].join('\n\n') + `\n\n${LINE}\n` };
}

/** Where the source of each library under the MPL is, as a section of the notices, or nothing when there is none. */
export function mplSources(packages) {
  const mpl = packages.filter((p) => /MPL/.test(p.license));
  if (!mpl.length) return [];
  const where = (p) =>
    p.kind === 'js'
      ? `https://www.npmjs.com/package/${p.name}/v/${p.version}`
      : p.source && !p.source.startsWith('registry+')
        ? p.source.replace(/^git\+/, '')
        : `https://crates.io/crates/${p.name}/${p.version}`;
  return [
    `${LINE}\nLibraries under the Mozilla Public License 2.0, used unmodified. Their source code is at:\n\n`
      + mpl.map((p) => `${p.name} ${p.version}  ${where(p)}`).join('\n'),
  ];
}
