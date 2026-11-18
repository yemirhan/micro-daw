/**
 * N7: an install that was interrupted (app quit mid-download, or a failure in
 * an earlier session) keeps its verified files in `.partial-<id>`. `packs:states`
 * reports such a pack as `{ state: 'error', message: INSTALL_PAUSED_MESSAGE }`
 * so its card shows "Resume" (Get then only downloads the missing files).
 * No Node imports: the renderer imports this constant too.
 */
export const INSTALL_PAUSED_MESSAGE = 'Download paused';
