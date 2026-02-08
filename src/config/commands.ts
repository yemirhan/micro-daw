import type { LucideIcon } from 'lucide-react';
import {
  Music,
  GraduationCap,
  Target,
  AudioLines,
  Play,
  CircleDot,
  Timer,
  Undo2,
  Redo2,
  Plus,
  Download,
  Maximize2,
  Piano,
  FilePlus,
  FolderOpen,
  Save,
  Clock,
  Settings,
  FileAudio,
  Terminal,
  Flag,
  SkipForward,
  SkipBack,
  FolderPlus,
  LayoutTemplate,
} from 'lucide-react';
import { SYNTH_PRESETS } from '@/utils/constants';
import type { AppMode } from '@/types/appMode';
import type { ProjectMeta } from '@/types/project';

export interface CommandDefinition {
  id: string;
  label: string;
  group: string;
  icon?: LucideIcon;
  shortcut?: string;
  keywords?: string;
  modes?: AppMode[];
  subtitle?: string;
}

export type CommandActions = Record<string, () => void>;

const STATIC_COMMANDS: CommandDefinition[] = [
  // Project
  { id: 'new-project', label: 'New Project', group: 'Project', icon: FilePlus, shortcut: '\u2318N', keywords: 'create new' },
  { id: 'open-project', label: 'Open Project', group: 'Project', icon: FolderOpen, shortcut: '\u2318O', keywords: 'open load' },
  { id: 'save', label: 'Save', group: 'Project', icon: Save, shortcut: '\u2318S' },
  { id: 'save-as', label: 'Save As', group: 'Project', icon: Save, shortcut: '\u2318\u21E7S', keywords: 'export save copy' },

  // Navigation
  { id: 'switch-daw', label: 'Switch to DAW', group: 'Navigation', icon: Music, shortcut: '\u23181', keywords: 'mode arrange perform' },
  { id: 'switch-learn', label: 'Switch to Learn', group: 'Navigation', icon: GraduationCap, shortcut: '\u23182', keywords: 'mode lessons tutorial' },
  { id: 'switch-practice', label: 'Switch to Practice', group: 'Navigation', icon: Target, shortcut: '\u23183', keywords: 'mode exercise drill' },
  { id: 'switch-samples', label: 'Switch to Samples', group: 'Navigation', icon: AudioLines, shortcut: '\u23184', keywords: 'mode samples audio library browse' },
  { id: 'switch-dev', label: 'Switch to Developer', group: 'Navigation', icon: Terminal, shortcut: '\u23185', keywords: 'mode developer midi monitor debug raw' },

  // Transport (DAW only)
  { id: 'play-stop', label: 'Play / Stop', group: 'Transport', icon: Play, shortcut: 'Space', modes: ['daw'], keywords: 'playback start pause' },
  { id: 'record', label: 'Record', group: 'Transport', icon: CircleDot, shortcut: 'R', modes: ['daw'], keywords: 'rec arm' },
  { id: 'toggle-metronome', label: 'Toggle Metronome', group: 'Transport', icon: Timer, shortcut: 'M', modes: ['daw'], keywords: 'click tempo beat' },

  // Editing
  { id: 'undo', label: 'Undo', group: 'Editing', icon: Undo2, shortcut: '\u2318Z' },
  { id: 'redo', label: 'Redo', group: 'Editing', icon: Redo2, shortcut: '\u2318\u21E7Z' },
  { id: 'add-synth-track', label: 'Add Synth Track', group: 'Editing', icon: Plus, modes: ['daw'], keywords: 'new track instrument' },
  { id: 'add-drum-track', label: 'Add Drum Track', group: 'Editing', icon: Plus, modes: ['daw'], keywords: 'new track percussion' },
  { id: 'add-audio-track', label: 'Add Audio Track', group: 'Editing', icon: FileAudio, modes: ['daw'], keywords: 'new track audio wav mp3' },
  { id: 'import-audio', label: 'Import Audio File', group: 'Editing', icon: FileAudio, shortcut: '\u2318I', modes: ['daw'], keywords: 'import audio wav mp3 sample' },
  { id: 'add-marker', label: 'Add Marker', group: 'Editing', icon: Flag, shortcut: '\u21E7M', modes: ['daw'], keywords: 'marker section flag' },
  { id: 'next-marker', label: 'Next Marker', group: 'Editing', icon: SkipForward, shortcut: ']', modes: ['daw'], keywords: 'marker jump next' },
  { id: 'prev-marker', label: 'Previous Marker', group: 'Editing', icon: SkipBack, shortcut: '[', modes: ['daw'], keywords: 'marker jump previous' },
  { id: 'create-group', label: 'Group Tracks', group: 'Editing', icon: FolderPlus, shortcut: '\u2318G', modes: ['daw'], keywords: 'group folder tracks' },
  { id: 'new-from-template', label: 'New from Template', group: 'Project', icon: LayoutTemplate, keywords: 'template preset project' },

  // Export (DAW only)
  { id: 'export-wav', label: 'Export as WAV', group: 'Export', icon: Download, shortcut: '\u2318E', modes: ['daw'], keywords: 'audio bounce render' },
  { id: 'export-midi', label: 'Export as MIDI', group: 'Export', icon: Download, shortcut: '\u2318E', modes: ['daw'], keywords: 'midi file' },

  // View
  { id: 'toggle-fullscreen', label: 'Toggle Fullscreen', group: 'View', icon: Maximize2, keywords: 'maximize screen' },
  { id: 'open-settings', label: 'Settings', group: 'View', icon: Settings, shortcut: '\u2318,', keywords: 'preferences options config' },

  // Instrument presets (generated from SYNTH_PRESETS)
  ...SYNTH_PRESETS.map((preset, i) => ({
    id: `preset-${i}`,
    label: preset.name,
    group: 'Instrument',
    icon: Piano,
    modes: ['daw'] as AppMode[],
    keywords: `synth preset sound ${preset.name.toLowerCase()} ${preset.oscillator.type}`,
  })),
];

export function getAvailableCommands(mode: AppMode): CommandDefinition[] {
  return STATIC_COMMANDS.filter((cmd) => !cmd.modes || cmd.modes.includes(mode));
}

export function buildRecentProjectCommands(recentProjects: ProjectMeta[]): CommandDefinition[] {
  return recentProjects.slice(0, 10).map((p, i) => ({
    id: `recent-${i}`,
    label: p.name,
    group: 'Recent Projects',
    icon: Clock,
    subtitle: p.filePath,
    keywords: `recent project ${p.name.toLowerCase()}`,
  }));
}
