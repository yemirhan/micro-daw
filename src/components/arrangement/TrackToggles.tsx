import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

type ToggleKind = 'mute' | 'solo' | 'arm';

const ON_CLASS: Record<ToggleKind, string> = {
  // Mute = amber, Solo = cyan-blue, Arm = record red (consistent everywhere).
  mute: 'bg-signal-warn text-black border-signal-warn',
  solo: 'bg-signal-select text-black border-signal-select',
  arm: 'bg-signal-rec text-white border-signal-rec',
};

interface TrackToggleProps {
  kind: ToggleKind;
  active: boolean;
  onClick: () => void;
  title: string;
  /** Recording in progress (arm button pulses). */
  pulsing?: boolean;
  size?: 'sm' | 'md';
  children?: ReactNode;
  className?: string;
}

/** Square M / S / ● toggle used by track headers, group headers and mixer strips. */
export function TrackToggle({ kind, active, onClick, title, pulsing, size = 'sm', children, className }: TrackToggleProps) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      title={title}
      aria-label={title}
      aria-pressed={active}
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-[4px] border font-mono font-bold leading-none',
        'transition-colors duration-100 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring',
        size === 'sm' ? 'h-[18px] min-w-[18px] px-1 text-[10px]' : 'h-6 min-w-6 px-1.5 text-[11px]',
        active
          ? ON_CLASS[kind]
          : 'border-border bg-surface-2 text-muted-foreground hover:border-input hover:bg-surface-3 hover:text-foreground',
        pulsing && 'animate-rec-pulse',
        className,
      )}
    >
      {children ??
        (kind === 'mute' ? 'M' : kind === 'solo' ? 'S' : (
          <span className={cn('rounded-full', size === 'sm' ? 'size-2' : 'size-2.5', active ? 'bg-white' : 'bg-signal-rec/80')} />
        ))}
    </button>
  );
}
