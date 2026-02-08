import type { Arrangement } from './arrangement';

export type TemplateCategory = 'rock' | 'hip-hop' | 'jazz' | 'edm' | 'ambient' | 'starter';

export interface ProjectTemplate {
  id: string;
  name: string;
  category: TemplateCategory;
  description: string;
  bpm: number;
  trackCount: number;
  arrangement: Arrangement;
}
