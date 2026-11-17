/**
 * Global vitest setup (see vitest.config.mts).
 *
 * - Adds @testing-library/jest-dom matchers (toBeInTheDocument, ...).
 * - Replaces `tone` with the test double from ./toneMock for every test file.
 *   Use `vi.unmock('tone')` at the top of a test file to opt out.
 * - Unmounts React trees rendered with @testing-library/react after each test.
 */
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';
import { resetToneMock } from './toneMock';

vi.mock('tone', async () => (await import('./toneMock')).createToneModule());

beforeEach(() => {
  resetToneMock();
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});
