import { ArrowLeft, ArrowRight, Check, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface TourTooltipProps {
  title: string;
  description: string;
  currentStep: number;
  totalSteps: number;
  onNext: () => void;
  onPrev: () => void;
  onSkip: () => void;
}

export function TourTooltip({
  title,
  description,
  currentStep,
  totalSteps,
  onNext,
  onPrev,
  onSkip,
}: TourTooltipProps) {
  const isLast = currentStep === totalSteps - 1;
  const isFirst = currentStep === 0;

  return (
    <div
      role="dialog"
      aria-modal="false"
      aria-labelledby="tour-title"
      aria-describedby="tour-description"
      className="w-80 rounded-xl border border-hairline-strong bg-popover p-4 text-popover-foreground shadow-[0_18px_48px_-16px_oklch(0_0_0/0.8)]"
    >
      <div className="flex items-center justify-between">
        <span className="font-mono text-[10.5px] font-semibold uppercase tracking-[0.1em] text-primary">
          Step {currentStep + 1} of {totalSteps}
        </span>
        <button
          type="button"
          onClick={onSkip}
          className="-mr-1 flex size-6 items-center justify-center rounded-md text-muted-foreground outline-none transition-colors hover:bg-surface-3 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/60"
          title="Skip tour (Esc)"
          aria-label="Skip tour"
        >
          <X className="size-3.5" />
        </button>
      </div>

      <h3 id="tour-title" className="mt-2 text-[15px] font-semibold text-foreground">
        {title}
      </h3>
      <p id="tour-description" className="mt-1.5 text-[13px] leading-relaxed text-muted-foreground">
        {description}
      </p>

      {/* Progress */}
      <div className="mt-4 flex items-center gap-1" aria-hidden>
        {Array.from({ length: totalSteps }, (_, i) => (
          <span
            key={i}
            className={cn(
              'h-1 flex-1 rounded-full transition-colors duration-200',
              i < currentStep ? 'bg-primary/50' : i === currentStep ? 'bg-primary' : 'bg-surface-4',
            )}
          />
        ))}
      </div>

      {/* Actions */}
      <div className="mt-4 flex items-center justify-between">
        <button
          type="button"
          onClick={onSkip}
          className="rounded text-xs text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/60"
        >
          Skip tour
        </button>
        <div className="flex gap-1.5">
          {!isFirst && (
            <Button variant="ghost" size="sm" className="h-7 px-2.5 text-xs" onClick={onPrev}>
              <ArrowLeft className="size-3.5" />
              Back
            </Button>
          )}
          <Button size="sm" className="h-7 px-3 text-xs font-semibold" onClick={onNext} autoFocus>
            {isLast ? (
              <>
                <Check className="size-3.5" />
                Done
              </>
            ) : (
              <>
                Next
                <ArrowRight className="size-3.5" />
              </>
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
