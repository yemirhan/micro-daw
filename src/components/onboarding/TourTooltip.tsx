import { Button } from '@/components/ui/button';

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
      className="w-72 rounded-xl border border-border/60 p-4 shadow-2xl"
      style={{ backgroundColor: 'oklch(0.16 0.02 270)' }}
    >
      <h3 className="text-sm font-semibold text-foreground">{title}</h3>
      <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">{description}</p>

      {/* Dot indicators */}
      <div className="mt-3 flex items-center gap-1">
        {Array.from({ length: totalSteps }, (_, i) => (
          <div
            key={i}
            className="h-1.5 rounded-full transition-all"
            style={{
              width: i === currentStep ? 16 : 6,
              backgroundColor: i === currentStep
                ? 'oklch(0.65 0.20 265)'
                : 'oklch(0.30 0.02 270)',
            }}
          />
        ))}
      </div>

      {/* Actions */}
      <div className="mt-3 flex items-center justify-between">
        <button
          onClick={onSkip}
          className="text-[11px] text-muted-foreground hover:text-foreground transition-colors"
        >
          Skip tour
        </button>
        <div className="flex gap-1.5">
          {!isFirst && (
            <Button variant="ghost" size="sm" className="h-7 px-2.5 text-xs" onClick={onPrev}>
              Back
            </Button>
          )}
          <Button
            size="sm"
            className="h-7 px-3 text-xs"
            onClick={onNext}
            style={{ backgroundColor: 'oklch(0.65 0.20 265)' }}
          >
            {isLast ? 'Done' : 'Next'}
          </Button>
        </div>
      </div>
    </div>
  );
}
