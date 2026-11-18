/**
 * Golden vectors for rune/crates/mdaw-model/src/settings.rs: every localStorage reader of the
 * Electron app over valid / invalid stored strings.
 *
 * Most readers are module-private functions inside components / hooks. They are not
 * reimplemented here: their exact source text is cut out of the real file, type-stripped with
 * the TypeScript compiler and evaluated with the same constants in scope, so the vectors come
 * from the shipped code.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { it } from 'vitest';
import { clampBrowserWidth, BROWSER_DEFAULT_WIDTH } from '@/components/browser/BrowserPanel';
import { findRootFor, normalizeRoots } from '@/components/browser/browserHelpers';
import { DEFAULT_SETTINGS, DEFAULT_VOLUME, MAX_VOLUME, MIN_VOLUME } from '@/utils/constants';
import { DEFAULT_NOTE_DURATION } from '@/utils/pianoRollHelpers';
import { kitStore } from '@/services/KitStore';
import { projectManager } from '@/services/ProjectManager';
import { writeFixture } from './fixture';

const SRC = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'src');

/** The source text of `function name(...) {...}` in `file`. */
function grab(src: string, name: string): string {
  const start = src.search(new RegExp(`function ${name}\\b`));
  if (start < 0) throw new Error(`no function ${name}`);
  let i = start;
  let paren = 0;
  for (; i < src.length; i++) {
    const c = src[i];
    if (c === '(') paren++;
    else if (c === ')') paren--;
    else if (c === '{' && paren === 0) break;
  }
  let depth = 0;
  for (let j = i; j < src.length; j++) {
    if (src[j] === '{') depth++;
    else if (src[j] === '}' && --depth === 0) return src.slice(start, j + 1);
  }
  throw new Error(`unbalanced ${name}`);
}

function load(file: string, names: string[], scope: Record<string, unknown>): Record<string, (...a: unknown[]) => unknown> {
  const src = readFileSync(join(SRC, file), 'utf8');
  const code = names.map((n) => grab(src, n)).join('\n');
  const js = ts.transpileModule(code, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  return new Function(...Object.keys(scope), `${js}\nreturn { ${names.join(', ')} };`)(...Object.values(scope));
}

/** Runs `read` with `key` holding `value` (null = not set). */
function withStored<T>(key: string, value: string | null, read: () => T): T {
  localStorage.clear();
  if (value !== null) localStorage.setItem(key, value);
  return read();
}

const COMMON = [null, '', ' ', 'null', 'undefined', '{nope', '0', '1', '-1', 'true', 'false', '"x"', '[]', '{}', 'NaN'];

it('dumps settings reader vectors', () => {
  const { readBrowserOpen } = load('app/App.tsx', ['readBrowserOpen'], { BROWSER_OPEN_KEY: 'micro-daw:browser-open' });
  const { readBank } = load('components/InstrumentDock.tsx', ['readBank'], { BANK_KEY: 'micro-daw:dock-pad-bank' });
  const { readPref } = load('components/beat/BeatView.tsx', ['readPref'], {});
  const piano = load('components/piano-roll-editor/PianoRollEditor.tsx', ['loadSettings'], { SETTINGS_KEY: 'micro-daw-piano-roll', DEFAULT_NOTE_DURATION });
  const appSettings = load('hooks/useSettings.ts', ['loadSettings'], { SETTINGS_STORAGE_KEY: 'micro-daw-settings', DEFAULT_SETTINGS });
  const { loadStoredVolume } = load('audio/masterBus.ts', ['clampVolume', 'loadStoredVolume'], {
    STORAGE_KEY: 'micro-daw:masterVolume',
    DEFAULT_VOLUME,
    MIN_VOLUME,
    MAX_VOLUME,
  });
  const { readWidth } = load('components/browser/BrowserPanel.tsx', ['readWidth'], { WIDTH_KEY: 'micro-daw:browser-width', clampBrowserWidth, BROWSER_DEFAULT_WIDTH });
  const browser = load('hooks/useSampleBrowser.ts', ['readStorage', 'loadRoots', 'loadLastPath'], {
    ROOTS_KEY: 'micro-daw:browser-roots',
    LAST_PATH_KEY: 'micro-daw:browser-last-path',
    normalizeRoots,
    findRootFor,
  });
  const { readRememberedNames } = load('services/PackService.ts', ['readRememberedNames'], { PACK_NAMES_STORAGE_KEY: 'micro-daw:pack-names' });

  const vec = (key: string, values: (string | null)[], read: () => unknown) => values.map((v) => [v, withStored(key, v, read)]);

  const browserOpen = vec('micro-daw:browser-open', [...COMMON, 'FALSE', 'False', ' false'], () => readBrowserOpen());
  const bank = vec('micro-daw:dock-pad-bank', [...COMMON, 'A', 'B', 'b', ' B', 'B '], () => readBank());
  const layout = vec('micro-daw:beat-pad-layout', [...COMMON, '4x4', '2x8', '2X8', ' 2x8'], () => readPref('micro-daw:beat-pad-layout', ['4x4', '2x8'], '4x4'));
  const grid = vec('micro-daw:beat-step-grid', [...COMMON, '1/8', '1/16', '1/32', '1/4', '1/8T'], () => readPref('micro-daw:beat-step-grid', ['1/8', '1/16', '1/32'], '1/16'));
  const keys = vec('micro-daw:beat-key-hints', [...COMMON, 'on', 'off', 'ON'], () => readPref('micro-daw:beat-key-hints', ['on', 'off'], 'off') === 'on');
  const pianoRoll = vec(
    'micro-daw-piano-roll',
    [
      ...COMMON,
      '{"snap":0.5,"drawLength":2,"follow":false}',
      '{"snap":0,"drawLength":0,"follow":"yes"}',
      '{"snap":-1,"drawLength":-2,"follow":null}',
      '{"snap":"0.5","drawLength":"2","follow":1}',
      '{"snap":1e3,"drawLength":0.001}',
      '{"follow":true,"extra":1}',
      '[0.5]',
      '42',
    ],
    () => piano.loadSettings(),
  );
  const settings = vec(
    'micro-daw-settings',
    [
      ...COMMON,
      '{"general":{"autoCheckUpdates":false,"onboarding":true},"audio":{"defaultMasterVolume":-6,"bufferSizeHint":512},"midi":{"autoConnectLastDevice":false}}',
      '{"general":{"autoCheckUpdates":null}}',
      '{"general":{"autoCheckUpdates":"no"}}',
      '{"general":"x","audio":"ab","midi":[true]}',
      '{"audio":{"bufferSizeHint":"big","extra":{"a":1},"10":1,"2":2}}',
      '{"audio":null,"midi":5}',
      '{"midi":{"autoConnectLastDevice":0}}',
      '{"oldFlag":true}',
      '[{"general":1}]',
    ],
    () => appSettings.loadSettings(),
  );
  const volume = vec('micro-daw:masterVolume', [...COMMON, '-20', '-12', '-40', '-41', '5', '-0', '0x10', '1e1', '-1e1', ' -7 ', '-7px', 'Infinity', '-Infinity', '-12.5', '.5', '-.5', '0b1', '1_0'], () => loadStoredVolume());
  const width = vec('micro-daw:browser-width', [...COMMON, '200', '199', '520', '521', '300.4', '300.5', '-300.5', '248', 'Infinity', '1e3', '0x100', ' 333 ', '350px', '-0'], () => readWidth());
  const rootsValues = [
    ...COMMON,
    '["/s"]',
    '["/s/","/s","/t\\\\",""," ",5,null,"C:\\\\","/"]',
    '["/a","/a/b","/a"]',
    '"just a string"',
    '{"0":"/a"}',
    '[["/nested"]]',
    '["\\u00a0"]',
  ];
  const roots = vec('micro-daw:browser-roots', rootsValues, () => browser.loadRoots());
  const lastPathRoots = [[], ['/s'], ['/s', '/s/sub'], ['C:\\music'], ['/']];
  const lastPaths: unknown[] = [];
  for (const rs of lastPathRoots) {
    for (const v of [null, '', '/s', '/s/', '/s/a/b', '/sx', '/t', 'C:\\music\\a', 'C:\\musicx', '/s/sub/x', 'relative']) {
      lastPaths.push([rs, v, withStored('micro-daw:browser-last-path', v, () => browser.loadLastPath(rs))]);
    }
  }
  const packNames = vec(
    'micro-daw:pack-names',
    [...COMMON, '{"a":"A","b":"","c":5,"d":null,"10":"Ten","2":"Two","e":"E"}', '{"__proto__":"P","x":"X"}', '[["a","A"]]', '"str"'],
    () => Object.entries(readRememberedNames() as Record<string, string>),
  );

  const kitsValues = [
    ...COMMON,
    '[{"id":"mine","name":"Mine","pads":[]}]',
    '[{"id":"","name":"","pads":[]},{"id":"b","name":"B"}]',
    '[null,1,"x",true,{"id":"ok","name":"Ok"},[]]',
    '{"id":"not-an-array"}',
  ];
  const kits = vec('micro-daw:kits', kitsValues, () => (kitStore.list() as { id: string; name: string; pads: unknown[] }[]).map((k) => ({ id: /^[0-9a-f-]{36}$/.test(k.id) ? '<uuid>' : k.id, name: k.name, pads: k.pads.length })));

  const pm = projectManager as unknown as { recentProjects: unknown; loadRecentProjects(): void };
  const recent = vec(
    'micro-daw-recent-projects',
    [...COMMON, '[{"filePath":"/a/b.mdaw","name":"b","modifiedAt":"2026-01-01"}]', '[{"filePath":5}]'],
    () => {
      pm.recentProjects = '<untouched>';
      pm.loadRecentProjects();
      return pm.recentProjects;
    },
  );

  writeFixture('settings', { browserOpen, bank, layout, grid, keys, pianoRoll, settings, volume, width, roots, lastPaths, packNames, kits, recent });
});
