import type { LucideIcon } from 'lucide-react';

interface StatCardProps {
  icon: LucideIcon;
  value: string | number;
  label: string;
  color?: string;
}

export function StatCard({ icon: Icon, value, label, color = 'oklch(0.65 0.20 265)' }: StatCardProps) {
  return (
    <div
      className="flex items-center gap-3 rounded-xl border border-border/50 p-4"
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <div
        className="flex h-10 w-10 items-center justify-center rounded-lg"
        style={{ backgroundColor: `color-mix(in oklch, ${color} 20%, transparent)` }}
      >
        <Icon className="h-5 w-5" style={{ color }} />
      </div>
      <div>
        <div className="text-lg font-bold tabular-nums">{value}</div>
        <div className="text-[11px] text-muted-foreground">{label}</div>
      </div>
    </div>
  );
}
