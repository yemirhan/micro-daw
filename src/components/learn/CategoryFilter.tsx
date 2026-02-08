import { cn } from '@/lib/utils';
import type { LessonCategory } from '@/types/appMode';

interface CategoryFilterProps {
  selected: LessonCategory | null;
  onChange: (category: LessonCategory | null) => void;
}

const CATEGORIES: { value: LessonCategory | null; label: string }[] = [
  { value: null, label: 'All' },
  { value: 'piano', label: 'Piano' },
  { value: 'drums', label: 'Drums' },
  { value: 'theory', label: 'Theory' },
  { value: 'ear-training', label: 'Ear Training' },
  { value: 'songs', label: 'Songs' },
];

export function CategoryFilter({ selected, onChange }: CategoryFilterProps) {
  return (
    <div className="flex gap-1">
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
