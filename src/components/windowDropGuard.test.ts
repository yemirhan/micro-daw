import { afterEach, describe, expect, it } from 'vitest';
import { installWindowDropGuard } from './windowDropGuard';

function dragEvent(type: 'dragover' | 'drop') {
  const e = new Event(type, { bubbles: true, cancelable: true }) as DragEvent;
  Object.defineProperty(e, 'dataTransfer', { value: { dropEffect: 'copy' } });
  return e;
}

describe('installWindowDropGuard', () => {
  let cleanup: (() => void) | null = null;
  afterEach(() => {
    cleanup?.();
    cleanup = null;
    document.body.innerHTML = '';
  });

  it('a drop outside any drop target never runs the browser default (navigation)', () => {
    cleanup = installWindowDropGuard();
    const over = dragEvent('dragover');
    document.body.dispatchEvent(over);
    expect(over.defaultPrevented).toBe(true);
    expect(over.dataTransfer!.dropEffect).toBe('none');
    const drop = dragEvent('drop');
    document.body.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(true);
  });

  it('leaves drop targets alone: their dropEffect stays and their drop handler runs', () => {
    cleanup = installWindowDropGuard();
    const pad = document.createElement('button');
    document.body.appendChild(pad);
    let dropped = 0;
    pad.addEventListener('dragover', (e) => {
      e.preventDefault();
      e.dataTransfer!.dropEffect = 'copy';
    });
    pad.addEventListener('drop', (e) => {
      e.preventDefault();
      dropped++;
    });
    const over = dragEvent('dragover');
    pad.dispatchEvent(over);
    expect(over.dataTransfer!.dropEffect).toBe('copy');
    pad.dispatchEvent(dragEvent('drop'));
    expect(dropped).toBe(1);
  });

  it('cleanup removes the guard', () => {
    installWindowDropGuard()();
    const drop = dragEvent('drop');
    document.body.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(false);
  });
});
