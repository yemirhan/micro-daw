// midi-writer-js ships types, but its package.json "exports" map doesn't expose
// them, so TypeScript's bundler resolution can't find them. It's only used via a
// dynamic import in ExportService (typed as any there).
declare module 'midi-writer-js';
