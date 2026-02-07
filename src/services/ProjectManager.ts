import { arrangementEngine } from './ArrangementEngine';
import { audioEngine } from './AudioEngine';
import { undoManager } from './UndoManager';
import type { ProjectFile, ProjectMeta } from '@/types/project';

const RECENT_PROJECTS_KEY = 'micro-daw-recent-projects';
const MAX_RECENT = 10;

type StateCallback = () => void;

class ProjectManager {
  private filePath: string | null = null;
  private isDirty = false;
  private recentProjects: ProjectMeta[] = [];
  private callbacks: StateCallback[] = [];
  private createdAt: string = new Date().toISOString();

  constructor() {
    this.loadRecentProjects();
  }

  // --- Getters ---

  getFilePath(): string | null {
    return this.filePath;
  }

  getIsDirty(): boolean {
    return this.isDirty;
  }

  getProjectName(): string {
    if (!this.filePath) return 'Untitled';
    const parts = this.filePath.split('/');
    const fileName = parts[parts.length - 1];
    return fileName.replace(/\.mdaw$/, '');
  }

  getRecentProjects(): ProjectMeta[] {
    return this.recentProjects;
  }

  // --- Actions ---

  async save(): Promise<boolean> {
    if (this.filePath) {
      await this.writeToFile(this.filePath);
      return true;
    }
    return await this.saveAs();
  }

  async saveAs(): Promise<boolean> {
    const api = window.electronAPI;
    if (!api) return false;

    const chosenPath = await api.projectShowSaveDialog(this.getProjectName() + '.mdaw');
    if (!chosenPath) return false;

    this.filePath = chosenPath;
    this.createdAt = new Date().toISOString();
    await this.writeToFile(chosenPath);
    return true;
  }

  async open(): Promise<boolean> {
    const shouldContinue = await this.promptUnsaved();
    if (!shouldContinue) return false;

    const api = window.electronAPI;
    if (!api) return false;

    const chosenPath = await api.projectShowOpenDialog();
    if (!chosenPath) return false;

    return await this.loadFromFile(chosenPath);
  }

  async openFromPath(path: string): Promise<boolean> {
    const shouldContinue = await this.promptUnsaved();
    if (!shouldContinue) return false;

    return await this.loadFromFile(path);
  }

  async newProject(): Promise<boolean> {
    const shouldContinue = await this.promptUnsaved();
    if (!shouldContinue) return false;

    this.filePath = null;
    this.isDirty = false;
    this.createdAt = new Date().toISOString();

    // Reset to default empty arrangement
    const defaultArrangement = {
      id: crypto.randomUUID(),
      name: 'Untitled',
      bpm: 120,
      lengthBeats: 64,
      tracks: [],
    };
    arrangementEngine.restoreFromSnapshot(defaultArrangement);
    undoManager.clear();

    // Reset audio config
    audioEngine.setVolume(-12);
    audioEngine.setReverbWet(0);
    audioEngine.setChorusDepth(0);
    audioEngine.setFilterCutoff(18000);
    audioEngine.setFilterResonance(1);

    this.updateWindowTitle();
    this.emit();
    return true;
  }

  markDirty(): void {
    if (this.isDirty) return;
    this.isDirty = true;
    this.updateWindowTitle();
    this.emit();
  }

  async handleBeforeClose(): Promise<void> {
    const api = window.electronAPI;
    if (!api) return;

    if (!this.isDirty) {
      api.forceClose();
      return;
    }

    const result = await api.projectShowUnsavedDialog(this.getProjectName());
    if (result === 'save') {
      const saved = await this.save();
      if (saved) {
        api.forceClose();
      }
    } else if (result === 'dont-save') {
      api.forceClose();
    }
    // 'cancel' — do nothing, keep window open
  }

  // --- Callbacks ---

  onStateChange(callback: StateCallback): () => void {
    this.callbacks.push(callback);
    return () => {
      this.callbacks = this.callbacks.filter((cb) => cb !== callback);
    };
  }

  // --- Private ---

  private async writeToFile(filePath: string): Promise<void> {
    const api = window.electronAPI;
    if (!api) return;

    const now = new Date().toISOString();
    const projectFile: ProjectFile = {
      version: '1.0.0',
      arrangement: arrangementEngine.getArrangement(),
      audioConfig: {
        masterVolume: audioEngine.getVolume(),
        effectParams: audioEngine.getEffectParams(),
      },
      createdAt: this.createdAt,
      modifiedAt: now,
    };

    await api.projectSave(filePath, JSON.stringify(projectFile, null, 2));
    this.isDirty = false;
    this.addToRecentProjects(filePath, now);
    this.updateWindowTitle();
    this.emit();
  }

  private async loadFromFile(filePath: string): Promise<boolean> {
    const api = window.electronAPI;
    if (!api) return false;

    try {
      const raw = await api.projectLoad(filePath);
      const data = JSON.parse(raw) as ProjectFile;

      if (data.version !== '1.0.0') {
        console.warn('Unknown project version:', data.version);
      }

      // Restore arrangement
      arrangementEngine.restoreFromSnapshot(data.arrangement);
      undoManager.clear();

      // Restore audio config
      if (data.audioConfig) {
        audioEngine.setVolume(data.audioConfig.masterVolume);
        const fx = data.audioConfig.effectParams;
        audioEngine.setReverbWet(fx.reverbWet);
        audioEngine.setChorusDepth(fx.chorusDepth);
        audioEngine.setFilterCutoff(fx.filterCutoff);
        audioEngine.setFilterResonance(fx.filterResonance);
      }

      this.filePath = filePath;
      this.isDirty = false;
      this.createdAt = data.createdAt || new Date().toISOString();
      this.addToRecentProjects(filePath, data.modifiedAt);
      this.updateWindowTitle();
      this.emit();
      return true;
    } catch (err) {
      console.error('Failed to load project:', err);
      return false;
    }
  }

  private async promptUnsaved(): Promise<boolean> {
    if (!this.isDirty) return true;

    const api = window.electronAPI;
    if (!api) return true;

    const result = await api.projectShowUnsavedDialog(this.getProjectName());
    if (result === 'save') {
      const saved = await this.save();
      return saved;
    }
    if (result === 'dont-save') {
      return true;
    }
    return false; // cancel
  }

  private updateWindowTitle(): void {
    const api = window.electronAPI;
    if (!api) return;
    const name = this.getProjectName();
    const dirty = this.isDirty ? ' *' : '';
    api.projectSetTitle(`${name}${dirty} - Micro DAW`);
  }

  private addToRecentProjects(filePath: string, modifiedAt: string): void {
    // Remove if already exists
    this.recentProjects = this.recentProjects.filter((p) => p.filePath !== filePath);

    const name = filePath.split('/').pop()?.replace(/\.mdaw$/, '') || 'Untitled';
    this.recentProjects.unshift({ filePath, name, modifiedAt });

    if (this.recentProjects.length > MAX_RECENT) {
      this.recentProjects = this.recentProjects.slice(0, MAX_RECENT);
    }

    this.saveRecentProjects();
  }

  private loadRecentProjects(): void {
    try {
      const raw = localStorage.getItem(RECENT_PROJECTS_KEY);
      if (raw) {
        this.recentProjects = JSON.parse(raw);
      }
    } catch {
      this.recentProjects = [];
    }
  }

  private saveRecentProjects(): void {
    try {
      localStorage.setItem(RECENT_PROJECTS_KEY, JSON.stringify(this.recentProjects));
    } catch {
      // ignore quota errors
    }
  }

  private emit(): void {
    for (const cb of this.callbacks) cb();
  }
}

export const projectManager = new ProjectManager();
