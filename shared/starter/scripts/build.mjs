// Builds the page into dist/ with the family's build: shared/scripts/build.mjs. Only what is this tool's own is here.
import { buildTool } from '../../scripts/build.mjs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

await buildTool({
  root: join(dirname(fileURLToPath(import.meta.url)), '..'),
  name: 'DotAny Starter',
  // the recent-files rows' icon, then the tool's own buttons
  icons: { file: 'file' },
});
