import { Music, GraduationCap, Target } from 'lucide-react';
import { SidebarIcon } from './SidebarIcon';
import type { AppMode } from '@/types/appMode';

interface SidebarProps {
  mode: AppMode;
  onModeChange: (mode: AppMode) => void;
}

const SIDEBAR_ITEMS: { mode: AppMode; icon: typeof Music; label: string }[] = [
  { mode: 'daw', icon: Music, label: 'DAW' },
  { mode: 'learn', icon: GraduationCap, label: 'Learn' },
  { mode: 'practice', icon: Target, label: 'Practice' },
];

export function Sidebar({ mode, onModeChange }: SidebarProps) {
  return (
    <div
      className="group/sidebar flex h-full w-12 shrink-0 flex-col overflow-hidden border-r border-border/50 transition-[width] duration-200 hover:w-[160px]"
      style={{
        backgroundColor: 'oklch(0.10 0.01 270)',
        WebkitAppRegion: 'drag',
      } as React.CSSProperties}
    >
      {/* Top padding for macOS traffic lights */}
      <div className="h-[38px] shrink-0" />

      <nav
        className="flex flex-col gap-1"
        style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
      >
        {SIDEBAR_ITEMS.map((item) => (
          <SidebarIcon
            key={item.mode}
            icon={item.icon}
            label={item.label}
            isActive={mode === item.mode}
            onClick={() => onModeChange(item.mode)}
          />
        ))}
      </nav>
    </div>
  );
}
