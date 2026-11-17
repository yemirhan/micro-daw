import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';

interface StatCardProps {
  icon: LucideIcon;
  value: ReactNode;
  label: string;
  /** Small context line under the value. */
  detail?: ReactNode;
  /** Kept for API compatibility; stat tiles are neutral. */
  color?: string;
}

/** Headline number tile. */
export function StatCard({ icon: Icon, value, label, detail }: StatCardProps) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-border bg-surface-1 px-4 py-3.5 shadow-[inset_0_1px_0_var(--hairline)]">
      <div className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
        <Icon className="size-3.5" aria-hidden />
        {label}
      </div>
      <div className="font-mono text-[26px] font-semibold leading-none tabular-nums text-foreground">{value}</div>
      {detail && <div className="text-xs text-muted-foreground">{detail}</div>}
    </div>
  );
}
