import type { ProjectTemplate, TemplateCategory } from '@/types/templates';

import blankTemplate from './blank-4track.json';
import rockTemplate from './rock-song.json';
import hipHopTemplate from './hip-hop-beat.json';
import jazzTemplate from './jazz-trio.json';
import edmTemplate from './edm-drop.json';
import ambientTemplate from './ambient-pad.json';

export const ALL_TEMPLATES: ProjectTemplate[] = [
  blankTemplate as unknown as ProjectTemplate,
  rockTemplate as unknown as ProjectTemplate,
  hipHopTemplate as unknown as ProjectTemplate,
  jazzTemplate as unknown as ProjectTemplate,
  edmTemplate as unknown as ProjectTemplate,
  ambientTemplate as unknown as ProjectTemplate,
];

export function getTemplatesByCategory(category: TemplateCategory): ProjectTemplate[] {
  return ALL_TEMPLATES.filter((t) => t.category === category);
}

export function getTemplateById(id: string): ProjectTemplate | undefined {
  return ALL_TEMPLATES.find((t) => t.id === id);
}
