import { useState, useCallback, useEffect } from 'react';
import { projectManager } from '@/services/ProjectManager';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { ProjectMeta } from '@/types/project';
import type { Arrangement } from '@/types/arrangement';

export function useProject() {
  const [filePath, setFilePath] = useState<string | null>(projectManager.getFilePath());
  const [isDirty, setIsDirty] = useState(projectManager.getIsDirty());
  const [projectName, setProjectName] = useState(projectManager.getProjectName());
  const [recentProjects, setRecentProjects] = useState<ProjectMeta[]>(projectManager.getRecentProjects());

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

  // Auto-mark dirty when arrangement changes, except the change an open / new makes when it
  // replaces the arrangement (`isReplacingProject`, synchronous around the replacement). The
  // hook used to suppress from the moment the operation started, before its dialogs: a
  // cancelled open / new swallowed the next edit (phase 10), and an edit made while a dialog
  // was open (a MIDI recording, a pad hit) was not marked dirty (phase 10 review). Electron
  // bugs fixed in both apps, user decision 2026-11-18.
  useEffect(() => {
    const unsub = arrangementEngine.onStateChange(() => {
      if (projectManager.isReplacingProject()) return;
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
    await projectManager.open();
  }, []);

  const newProject = useCallback(async () => {
    await projectManager.newProject();
  }, []);

  const openRecent = useCallback(async (path: string) => {
    await projectManager.openFromPath(path);
  }, []);

  const newProjectFromTemplate = useCallback(async (arrangement: Arrangement) => {
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
