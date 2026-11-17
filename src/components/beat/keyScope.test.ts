import { describe, expect, it } from 'vitest';
import { isScopedKeyTarget } from './keyScope';

function mount(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  document.body.appendChild(root);
  return root;
}

describe('isScopedKeyTarget', () => {
  it('keeps performance keys out of the sample browser, text fields, dialogs and menus', () => {
    const root = mount(`
      <aside data-keys-scope="browser"><div role="listbox" tabindex="0" id="list"></div><button id="add">Add</button></aside>
      <input id="bpm" /><textarea id="ta"></textarea><select id="sel"></select>
      <div role="dialog"><button id="dlg">ok</button></div>
      <div role="menu"><div id="mi" role="menuitem"></div></div>
      <button id="pad">pad</button><canvas id="grid" tabindex="0"></canvas>
    `);
    const $ = (id: string) => root.querySelector(`#${id}`);
    for (const id of ['list', 'add', 'bpm', 'ta', 'sel', 'dlg', 'mi']) expect(isScopedKeyTarget($(id))).toBe(true);
    expect(isScopedKeyTarget($('pad'))).toBe(false);
    expect(isScopedKeyTarget($('grid'))).toBe(false);
    expect(isScopedKeyTarget(document.body)).toBe(false);
    expect(isScopedKeyTarget(null)).toBe(false);
    root.remove();
  });
});
