import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface ChartCardProps {
  title: string;
  subtitle?: string;
  /** Headline value shown right of the title. */
  value?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** Card frame shared by the stats widgets. */
export function ChartCard({ title, subtitle, value, children, className }: ChartCardProps) {
  return (
    <section
      className={cn(
        'flex flex-col rounded-xl border border-border bg-surface-1 p-4 shadow-[inset_0_1px_0_var(--hairline)]',
        className,
      )}
      aria-label={title}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h3 className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">{title}</h3>
          {subtitle && <p className="mt-0.5 text-xs text-muted-foreground/80">{subtitle}</p>}
        </div>
        {value !== undefined && (
          <div className="text-right font-mono text-lg font-semibold tabular-nums leading-none text-foreground">{value}</div>
        )}
      </div>
      {children}
    </section>
  );
}

export function ChartEmpty({ children }: { children: ReactNode }) {
  return (
    <div className="flex flex-1 items-center justify-center rounded-lg border border-dashed border-border py-10 text-center text-xs text-muted-foreground">
      {children}
    </div>
  );
}
