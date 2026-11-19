import { useState, useCallback, useEffect, useRef } from 'react';
import { projectManager } from '@/services/ProjectManager';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { ProjectMeta } from '@/types/project';
import type { Arrangement } from '@/types/arrangement';

export function useProject() {
  const [filePath, setFilePath] = useState<string | null>(projectManager.getFilePath());
  const [isDirty, setIsDirty] = useState(projectManager.getIsDirty());
  const [projectName, setProjectName] = useState(projectManager.getProjectName());
  const [recentProjects, setRecentProjects] = useState<ProjectMeta[]>(projectManager.getRecentProjects());
  const skipDirtyRef = useRef(false);

  // Sync state from ProjectManager
  const syncState = useCallback(() => {
    setFilePath(projectManager.getFilePath());
    setIsDirty(projectManager.getIsDirty());
    setProjectName(projectManager.getProjectName());
    setRecentProjects([...projectManager.getRecentProjects()]);
  }, []);

  useEffect(() => {
    return projectManager.onStateChange(syncState);
  }, [syncState]);

  // Auto-mark dirty when arrangement changes. `skipDirtyRef` covers the open / new itself
  // (its state changes arrive synchronously while it runs) and is cleared when it ends, so a
  // cancelled open / new does not swallow the next real edit (Electron bug fixed in both apps,
  // Rune port phase 10, user decision 2026-11-18).
  useEffect(() => {
    const unsub = arrangementEngine.onStateChange(() => {
      if (skipDirtyRef.current) {
        skipDirtyRef.current = false;
        return;
      }
      projectManager.markDirty();
    });
    return unsub;
  }, []);

  // Window close guard
  useEffect(() => {
    const api = window.electronAPI;
    if (!api) return;
    return api.onBeforeClose(() => {
      projectManager.handleBeforeClose();
    });
  }, []);

  const save = useCallback(async () => {
    await projectManager.save();
  }, []);

  const saveAs = useCallback(async () => {
    await projectManager.saveAs();
  }, []);

  const open = useCallback(async () => {
    skipDirtyRef.current = true;
    try {
      await projectManager.open();
    } finally {
      skipDirtyRef.current = false;
    }
  }, []);

  const newProject = useCallback(async () => {
    skipDirtyRef.current = true;
    try {
      await projectManager.newProject();
    } finally {
      skipDirtyRef.current = false;
    }
  }, []);

  const openRecent = useCallback(async (path: string) => {
    skipDirtyRef.current = true;
    try {
      await projectManager.openFromPath(path);
    } finally {
      skipDirtyRef.current = false;
    }
  }, []);

  const newProjectFromTemplate = useCallback(async (arrangement: Arrangement) => {
    skipDirtyRef.current = true;
    try {
      await projectManager.newProjectFromTemplate(arrangement);
    } finally {
      skipDirtyRef.current = false;
    }
  }, []);

  return {
    filePath,
    isDirty,
    projectName,
    recentProjects,
    save,
    saveAs,
    open,
    newProject,
    openRecent,
    newProjectFromTemplate,
  };
}
