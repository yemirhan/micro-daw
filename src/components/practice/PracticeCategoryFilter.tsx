import { cn } from '@/lib/utils';
import type { PracticeCategory } from '@/types/appMode';

interface PracticeCategoryFilterProps {
  selected: PracticeCategory | null;
  onChange: (category: PracticeCategory | null) => void;
}

const CATEGORIES: { value: PracticeCategory | null; label: string }[] = [
  { value: null, label: 'All' },
  { value: 'piano-basics', label: 'Piano Basics' },
  { value: 'scales', label: 'Scales' },
  { value: 'chords', label: 'Chords' },
  { value: 'rhythm', label: 'Rhythm' },
  { value: 'drum-patterns', label: 'Drum Patterns' },
];

export function PracticeCategoryFilter({ selected, onChange }: PracticeCategoryFilterProps) {
  return (
    <div className="flex flex-wrap gap-1">
      {CATEGORIES.map((cat) => (
        <button
          key={cat.label}
          onClick={() => onChange(cat.value)}
          className={cn(
            'rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
            selected === cat.value
              ? 'bg-primary text-primary-foreground'
              : 'bg-secondary text-muted-foreground hover:text-foreground',
          )}
        >
          {cat.label}
        </button>
      ))}
    </div>
  );
}
