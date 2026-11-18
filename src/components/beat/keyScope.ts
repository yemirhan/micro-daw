import { isTypingTarget } from './beatHelpers';

/**
 * Keyboard scopes: a subtree marked `data-keys-scope="<name>"` owns its own
 * single-key bindings (the sample browser's type-to-filter, list navigation…),
 * so the beat view's performance keys (pads, R) must stay out of it.
 */
export const KEYS_SCOPE_ATTR = 'data-keys-scope';
export const BROWSER_KEYS_SCOPE = 'browser';

const OWNED_SELECTOR = [
  `[${KEYS_SCOPE_ATTR}]`,
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[role="menu"]',
  '[role="listbox"]',
].join(', ');

/**
 * True when a key event on `target` belongs to whatever has focus rather than
 * to the beat view's global performance keys: text fields (inputs, textareas,
 * selects, contenteditable), anything inside a keys scope (the sample browser),
 * dialogs, menus and listboxes.
 */
export function isScopedKeyTarget(target: EventTarget | null): boolean {
  if (isTypingTarget(target)) return true;
  if (!target || typeof (target as Element).closest !== 'function') return false;
  return !!(target as Element).closest(OWNED_SELECTOR);
}
