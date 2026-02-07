import type { LucideIcon } from 'lucide-react';

interface PracticeActivityCardProps {
  icon: LucideIcon;
  title: string;
  description: string;
  color: string;
  onClick: () => void;
}

export function PracticeActivityCard({ icon: Icon, title, description, color, onClick }: PracticeActivityCardProps) {
  return (
    <button
      onClick={onClick}
      className="flex flex-col gap-3 rounded-xl border border-border/50 p-5 text-left transition-all duration-150 hover:border-border hover:bg-card/60"
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <div
        className="flex h-10 w-10 items-center justify-center rounded-lg"
        style={{ backgroundColor: `color-mix(in oklch, ${color} 20%, transparent)` }}
      >
        <Icon className="h-5 w-5" style={{ color }} />
      </div>
      <div>
        <h3 className="text-sm font-semibold text-foreground">{title}</h3>
        <p className="mt-1 text-xs text-muted-foreground">{description}</p>
      </div>
    </button>
  );
}
