import type { AppMode } from './appMode';

export interface TourStep {
  target: string;           // data-tour="..." selector value
  title: string;
  description: string;
  placement: 'top' | 'bottom' | 'left' | 'right';
  requiredMode?: AppMode;   // auto-switch mode before showing
}
