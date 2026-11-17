import './fonts';
import './index.css';
import { createRoot } from 'react-dom/client';
import { createElement } from 'react';
import { App } from './app/App';

if (import.meta.env.DEV) {
  // Dev-only window.__microDaw handle (MIDI simulation, service access). Tree-shaken in prod.
  void import('./dev/debugHandle').then((m) => m.installDebugHandle());
}

const root = createRoot(document.getElementById('root')!);
root.render(createElement(App));
