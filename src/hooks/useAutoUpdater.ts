import { useCallback, useEffect, useRef, useState } from 'react';
import type { UpdateStatus } from '@/types/settings';

export function useAutoUpdater(autoCheck: boolean) {
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ state: 'idle' });
  const [appVersion, setAppVersion] = useState<string>('');
  const checkedOnMount = useRef(false);

  useEffect(() => {
    const api = window.electronAPI;
    if (!api?.getAppVersion) return;
    api.getAppVersion().then(setAppVersion);
  }, []);

  useEffect(() => {
    const api = window.electronAPI;
    if (!api?.onUpdaterStatus) return;
    const unsub = api.onUpdaterStatus((status: UpdateStatus) => {
      setUpdateStatus(status);
    });
    return unsub;
  }, []);

  useEffect(() => {
    if (autoCheck && !checkedOnMount.current) {
      checkedOnMount.current = true;
      window.electronAPI?.updaterCheck?.();
    }
  }, [autoCheck]);

  const checkForUpdates = useCallback(() => {
    setUpdateStatus({ state: 'checking' });
    window.electronAPI?.updaterCheck?.();
  }, []);

  const downloadUpdate = useCallback(() => {
    window.electronAPI?.updaterDownload?.();
  }, []);

  const installUpdate = useCallback(() => {
    window.electronAPI?.updaterInstall?.();
  }, []);

  return { updateStatus, checkForUpdates, downloadUpdate, installUpdate, appVersion };
}
