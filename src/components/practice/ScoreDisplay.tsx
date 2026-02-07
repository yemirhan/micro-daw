import { Star, RotateCcw, ArrowLeft } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface ScoreDisplayProps {
  accuracy: number;
  stars: 0 | 1 | 2 | 3;
  onRetry: () => void;
  onBack: () => void;
}

const MESSAGES: Record<number, string> = {
  3: 'Excellent! Perfect score!',
  2: 'Great job! Keep practicing!',
  1: 'Good start! Try again for more stars!',
  0: 'Keep trying! You can do it!',
};

export function ScoreDisplay({ accuracy, stars, onRetry, onBack }: ScoreDisplayProps) {
  return (
    <div className="flex flex-col items-center gap-6 p-8">
      <h2 className="text-lg font-bold">Exercise Complete!</h2>

      <div className="flex gap-2">
        {[1, 2, 3].map((i) => (
          <Star
            key={i}
            className={cn(
              'h-10 w-10 transition-all',
              i <= stars
                ? 'fill-yellow-400 text-yellow-400 drop-shadow-[0_0_8px_oklch(0.80_0.18_90)]'
                : 'text-muted-foreground/20',
            )}
          />
        ))}
      </div>

      <div className="text-center">
        <div className="text-3xl font-bold">{Math.round(accuracy * 100)}%</div>
        <p className="mt-1 text-sm text-muted-foreground">{MESSAGES[stars]}</p>
      </div>

      <div className="flex gap-3">
        <Button variant="outline" size="sm" onClick={onRetry}>
          <RotateCcw className="mr-1 h-3.5 w-3.5" />
          Try Again
        </Button>
        <Button variant="default" size="sm" onClick={onBack}>
          <ArrowLeft className="mr-1 h-3.5 w-3.5" />
          Back to Exercises
        </Button>
      </div>
    </div>
  );
}
