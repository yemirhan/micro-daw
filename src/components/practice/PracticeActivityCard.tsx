import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

interface PracticeActivityCardProps {
  icon: LucideIcon;
  title: string;
  description: string;
  /** Kept for API compatibility; tiles use the shared accent instead. */
  color?: string;
  onClick: () => void;
}

export function PracticeActivityCard({ icon: Icon, title, description, onClick }: PracticeActivityCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'group flex items-start gap-3 rounded-xl border border-border bg-surface-1 p-3.5 text-left',
        'shadow-[inset_0_1px_0_var(--hairline)] transition-[border-color,background-color,transform] duration-150',
        'hover:border-primary/40 hover:bg-surface-2 active:translate-y-px',
        'outline-none focus-visible:ring-2 focus-visible:ring-ring/60',
      )}
    >
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-surface-2 text-muted-foreground transition-colors duration-150 group-hover:border-primary/40 group-hover:text-primary">
        <Icon className="size-4.5" aria-hidden />
      </span>
      <span className="min-w-0">
        <span className="block text-sm font-semibold text-foreground">{title}</span>
        <span className="mt-0.5 block text-xs leading-snug text-muted-foreground">{description}</span>
      </span>
    </button>
  );
}
