import { describe, expect, it } from 'vitest';
import { assertTrustedSender, isTrustedRendererUrl } from './sender';

const DEV = 'http://localhost:5173';
const PROD = 'file:///Applications/Micro%20DAW.app/Contents/Resources/app.asar/.vite/renderer/main_window/index.html';

describe('isTrustedRendererUrl', () => {
  it('accepts the dev server origin (any path / hash)', () => {
    expect(isTrustedRendererUrl('http://localhost:5173/', DEV)).toBe(true);
    expect(isTrustedRendererUrl('http://localhost:5173/index.html#/packs', DEV)).toBe(true);
  });

  it('rejects other origins, ports and protocols', () => {
    expect(isTrustedRendererUrl('http://localhost:5174/', DEV)).toBe(false);
    expect(isTrustedRendererUrl('https://localhost:5173/', DEV)).toBe(false);
    expect(isTrustedRendererUrl('https://evil.example/', DEV)).toBe(false);
    expect(isTrustedRendererUrl('file:///tmp/index.html', DEV)).toBe(false);
  });

  it('accepts only the packaged index.html for file: renderers', () => {
    expect(isTrustedRendererUrl(PROD, PROD)).toBe(true);
    expect(isTrustedRendererUrl(`${PROD}#/beat`, PROD)).toBe(true);
    expect(isTrustedRendererUrl('file:///tmp/evil.html', PROD)).toBe(false);
    expect(isTrustedRendererUrl(PROD.replace('index.html', 'other.html'), PROD)).toBe(false);
    expect(isTrustedRendererUrl('https://raw.githubusercontent.com/x/index.html', PROD)).toBe(false);
  });

  it('rejects missing or malformed URLs', () => {
    expect(isTrustedRendererUrl(undefined, DEV)).toBe(false);
    expect(isTrustedRendererUrl('', DEV)).toBe(false);
    expect(isTrustedRendererUrl('not a url', DEV)).toBe(false);
    expect(isTrustedRendererUrl(DEV, 'not a url')).toBe(false);
  });
});

describe('assertTrustedSender', () => {
  it('passes the main frame of the app renderer', () => {
    expect(() => assertTrustedSender({ senderFrame: { url: `${DEV}/`, parent: null } }, DEV)).not.toThrow();
  });

  it('throws for subframes, foreign pages and a missing (navigated / destroyed) frame', () => {
    expect(() => assertTrustedSender({ senderFrame: { url: `${DEV}/`, parent: {} } }, DEV)).toThrow('Untrusted IPC sender');
    expect(() => assertTrustedSender({ senderFrame: { url: 'https://evil.example/', parent: null } }, DEV)).toThrow('Untrusted IPC sender');
    expect(() => assertTrustedSender({ senderFrame: null }, DEV)).toThrow('Untrusted IPC sender');
    expect(() => assertTrustedSender({}, DEV)).toThrow('Untrusted IPC sender');
  });
});
