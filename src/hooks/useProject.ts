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

  // Auto-mark dirty when arrangement changes
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
    await projectManager.open();
  }, []);

  const newProject = useCallback(async () => {
    skipDirtyRef.current = true;
    await projectManager.newProject();
  }, []);

  const openRecent = useCallback(async (path: string) => {
    skipDirtyRef.current = true;
    await projectManager.openFromPath(path);
  }, []);

  const newProjectFromTemplate = useCallback(async (arrangement: Arrangement) => {
    skipDirtyRef.current = true;
    await projectManager.newProjectFromTemplate(arrangement);
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
