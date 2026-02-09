import { arrangementEngine } from './ArrangementEngine';
import { audioEngine } from './AudioEngine';
import { undoManager } from './UndoManager';
import { sampleManager } from './SampleManager';
import type { ProjectFile, ProjectMeta, SampleManifestEntry } from '@/types/project';
import type { Arrangement } from '@/types/arrangement';

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
    const api = window.electronAPI;
    if (!api) return false;

    const shouldContinue = await this.promptUnsaved();
    if (!shouldContinue) return false;

    // Ask where to save the new project
    const chosenPath = await api.projectShowSaveDialog('Untitled.mdaw');
    if (!chosenPath) return false;

    this.createdAt = new Date().toISOString();

    // Reset to default empty arrangement
    const defaultArrangement = {
      id: crypto.randomUUID(),
      name: 'Untitled',
      bpm: 120,
      lengthBeats: 64,
      tracks: [] as any[],
    };
    arrangementEngine.restoreFromSnapshot(defaultArrangement);
    undoManager.clear();

    // Reset audio config
    audioEngine.setVolume(-12);
    audioEngine.setReverbWet(0);
    audioEngine.setChorusDepth(0);
    audioEngine.setFilterCutoff(18000);
    audioEngine.setFilterResonance(1);

    // Save to chosen path
    this.filePath = chosenPath;
    this.isDirty = false;
    await this.writeToFile(chosenPath);
    return true;
  }

  async newProjectFromTemplate(arrangement: Arrangement): Promise<boolean> {
    const api = window.electronAPI;
    if (!api) return false;

    const shouldContinue = await this.promptUnsaved();
    if (!shouldContinue) return false;

    const chosenPath = await api.projectShowSaveDialog('Untitled.mdaw');
    if (!chosenPath) return false;

    this.createdAt = new Date().toISOString();

    // Load template arrangement
    arrangementEngine.loadFromTemplate(arrangement);
    undoManager.clear();

    // Reset audio config
    audioEngine.setVolume(-12);
    audioEngine.setReverbWet(0);
    audioEngine.setChorusDepth(0);
    audioEngine.setFilterCutoff(18000);
    audioEngine.setFilterResonance(1);

    this.filePath = chosenPath;
    this.isDirty = false;
    await this.writeToFile(chosenPath);
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
    const arrangement = arrangementEngine.getArrangement();

    // Build sample manifest from audio regions
    const sampleManifest: SampleManifestEntry[] = [];
    const seenSampleIds = new Set<string>();
    for (const track of arrangement.tracks) {
      for (const region of track.regions) {
        if (region.audio && !seenSampleIds.has(region.audio.sampleId)) {
          seenSampleIds.add(region.audio.sampleId);
          const sample = sampleManager.getSample(region.audio.sampleId);
          sampleManifest.push({
            sampleId: region.audio.sampleId,
            path: region.audio.samplePath,
            name: sample?.name ?? region.name ?? 'Sample',
          });
        }
      }
    }

    const projectFile: ProjectFile = {
      version: '1.0.0',
      arrangement,
      audioConfig: {
        masterVolume: audioEngine.getVolume(),
        effectParams: audioEngine.getEffectParams(),
      },
      createdAt: this.createdAt,
      modifiedAt: now,
      ...(sampleManifest.length > 0 ? { sampleManifest } : {}),
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

      // Restore samples from manifest (before restoring arrangement, so buffers are available)
      if (data.sampleManifest) {
        for (const entry of data.sampleManifest) {
          const result = await sampleManager.loadFromPathWithId(entry.sampleId, entry.path);
          if (!result) {
            console.warn(`ProjectManager: could not load sample "${entry.name}" from ${entry.path}`);
          }
        }
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
