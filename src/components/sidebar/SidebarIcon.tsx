import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

interface SidebarIconProps {
  icon: LucideIcon;
  label: string;
  isActive: boolean;
  onClick: () => void;
}

export function SidebarIcon({ icon: Icon, label, isActive, onClick }: SidebarIconProps) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'group/item relative flex w-full items-center gap-3 px-3 py-2.5',
        'transition-colors duration-150',
        isActive
          ? 'text-foreground'
          : 'text-muted-foreground hover:text-foreground',
      )}
      title={label}
    >
      {/* Active accent bar */}
      {isActive && (
        <div
          className="absolute left-0 top-1/2 h-5 w-[3px] -translate-y-1/2 rounded-r-full"
          style={{ backgroundColor: 'oklch(0.65 0.25 270)' }}
        />
      )}

      <Icon className="h-5 w-5 shrink-0" />

      <span className="truncate text-xs font-medium opacity-0 transition-opacity duration-200 group-hover/sidebar:opacity-100">
        {label}
      </span>
    </button>
  );
}
