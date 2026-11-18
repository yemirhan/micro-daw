import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), '..', 'fixtures');

/** Write `rune/parity/fixtures/<name>.json` (stable key order as produced). */
export function writeFixture(name: string, data: unknown): void {
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(join(FIXTURES, `${name}.json`), JSON.stringify(data) + '\n');
}
